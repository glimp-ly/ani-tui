// ============================================================
// src/scraper/search.rs — Búsqueda de anime
// ============================================================
//
// Realiza búsquedas en el catálogo de animeav1.com extrayendo
// los datos del JSON embebido en el HTML de la página.
//
// El sitio usa Nuxt.js SSR y los resultados de búsqueda vienen
// directamente en el HTML inicial como datos de hidratación,
// en un bloque JavaScript con formato:
//   results:[{id:"...",title:"...",slug:"...",categoryId:N,...}]
//
// NO se necesita JavaScript ni headless Chrome.
// ============================================================

use anyhow::Result;
use regex::Regex;
use std::sync::OnceLock;

use crate::config::BASE_URL;
use crate::logger;
use crate::scraper::client::fetch_html;
use crate::scraper::{find_balanced_content, iter_top_level_objects, unescape_js_string};
use crate::security;
use crate::structs::{truncate_chars, Anime, AnimeCategory};

// ————————————————————————————————————————————————
// Patrones regex (compilados una sola vez al inicio)
// ————————————————————————————————————————————————

/// Extrae el campo `id` de un objeto de resultado.
static RE_FIELD_ID: OnceLock<Regex> = OnceLock::new();

/// Extrae el campo `title` de un objeto de resultado.
static RE_FIELD_TITLE: OnceLock<Regex> = OnceLock::new();

/// Extrae el campo `slug` de un objeto de resultado.
/// NOTA: Puede haber múltiples slugs (del anime y de la categoría).
/// Se toma el primero, que corresponde al slug del anime.
static RE_FIELD_SLUG: OnceLock<Regex> = OnceLock::new();

/// Extrae el campo `synopsis` de un objeto de resultado.
static RE_FIELD_SYNOPSIS: OnceLock<Regex> = OnceLock::new();

/// Extrae el campo `categoryId` de un objeto de resultado.
static RE_FIELD_CATEGORY_ID: OnceLock<Regex> = OnceLock::new();

/// Extrae el total de páginas del bloque de paginación.
static RE_TOTAL_PAGES: OnceLock<Regex> = OnceLock::new();

/// Extrae el total de registros del bloque de paginación.
static RE_TOTAL_RECORDS: OnceLock<Regex> = OnceLock::new();

// ————————————————————————————————————————————————
// Inicialización de regexes
// ————————————————————————————————————————————————

fn get_re_field_id() -> &'static Regex {
    RE_FIELD_ID.get_or_init(|| {
        // id:"197" — ID del anime como string entre comillas
        Regex::new(r#"\bid:"(\d+)""#).expect("regex field_id inválida")
    })
}

fn get_re_field_title() -> &'static Regex {
    RE_FIELD_TITLE.get_or_init(|| {
        Regex::new(r#"title:"((?:[^"\\]|\\.)*)""#).expect("regex field_title inválida")
    })
}

fn get_re_field_slug() -> &'static Regex {
    RE_FIELD_SLUG.get_or_init(|| {
        // slug:"one-piece" — toma el primer slug encontrado en el objeto
        Regex::new(r#"slug:"((?:[^"\\]|\\.)*)""#).expect("regex field_slug inválida")
    })
}

fn get_re_field_synopsis() -> &'static Regex {
    RE_FIELD_SYNOPSIS.get_or_init(|| {
        Regex::new(r#"synopsis:"((?:[^"\\]|\\.)*)""#).expect("regex field_synopsis inválida")
    })
}

fn get_re_field_category_id() -> &'static Regex {
    RE_FIELD_CATEGORY_ID.get_or_init(|| {
        Regex::new(r"categoryId:(\d+)").expect("regex field_category_id inválida")
    })
}

fn get_re_total_pages() -> &'static Regex {
    RE_TOTAL_PAGES.get_or_init(|| {
        Regex::new(r"totalPages:(\d+)").expect("regex total_pages inválida")
    })
}

fn get_re_total_records() -> &'static Regex {
    RE_TOTAL_RECORDS.get_or_init(|| {
        Regex::new(r"totalRecords:(\d+)").expect("regex total_records inválida")
    })
}

// ————————————————————————————————————————————————
// Información de paginación
// ————————————————————————————————————————————————

/// Información de paginación de los resultados de búsqueda.
#[derive(Debug, Clone)]
pub struct SearchPagination {
    /// Página actual (base 1)
    pub current_page: u32,
    /// Total de páginas disponibles
    pub total_pages: u32,
    /// Total de resultados para la búsqueda
    pub total_records: u32,
}

/// Resultado completo de una búsqueda, incluyendo animes y paginación.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Lista de animes encontrados en esta página
    pub animes: Vec<Anime>,
    /// Información de paginación
    pub pagination: SearchPagination,
}

// ————————————————————————————————————————————————
// Función principal de búsqueda
// ————————————————————————————————————————————————

/// Busca animes en el catálogo de animeav1.com.
///
/// # Parámetros
/// - `query`: Texto de búsqueda (ej: "one piece", "naruto")
/// - `page`: Número de página (base 1, default 1)
///
/// # Retorna
/// `SearchResult` con la lista de animes encontrados y datos de paginación.
///
/// # Errores
/// - Error de red si el sitio no responde
/// - Error de parseo si la estructura del HTML cambió
///
/// # Ejemplo
/// ```rust
/// let result = search_anime("one piece", 1).await?;
/// println!("Encontrados: {} animes", result.animes.len());
/// for anime in &result.animes {
///     println!("[{}] {} ({})", anime.slug, anime.title, anime.category.display());
/// }
/// ```
pub async fn search_anime(query: &str, page: u32) -> Result<SearchResult> {
    security::validate_search_query(query)
        .map_err(|e| anyhow::anyhow!("Consulta de búsqueda inválida: {}", e))?;

    let page = page.max(1);

    // Construir URL de búsqueda con paginación
    let url = format!(
        "{}/catalogo?search={}&page={}",
        BASE_URL,
        urlencoding_simple(query.trim()),
        page
    );

    let html = fetch_html(&url).await?;

    // Parsear los resultados del HTML
    parse_search_results(&html, page)
}

/// Codificación URL compatible con UTF-8 para términos de búsqueda.
/// Reemplaza espacios por '+' y codifica byte a byte en porcentaje `%XX`.
fn urlencoding_simple(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b' ' => encoded.push('+'),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            byte => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

// ————————————————————————————————————————————————
// Parseo del HTML
// ————————————————————————————————————————————————

/// Parsea los resultados de búsqueda del HTML del catálogo.
///
/// Estrategia:
/// 1. Localiza el bloque `results:[...]` mediante conteo balanceado respetando comillas
/// 2. Itera los objetos de primer nivel del array sin copiar ni reemplazar strings
/// 3. Para cada objeto, extrae y desescapa los campos con regex
/// 4. Extrae datos de paginación del HTML
pub fn parse_search_results(html: &str, current_page: u32) -> Result<SearchResult> {
    let results_block = find_balanced_content(html, "results:[", b'[', b']').unwrap_or("");

    // Si no hay resultados, devolver lista vacía con paginación vacía
    if results_block.trim().is_empty() {
        return Ok(SearchResult {
            animes: vec![],
            pagination: SearchPagination {
                current_page,
                total_pages: 0,
                total_records: 0,
            },
        });
    }

    // — Dividir en objetos individuales y parsear cada uno —
    let animes = parse_anime_objects(results_block);

    // — Extraer paginación —
    let total_pages = get_re_total_pages()
        .captures(html)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(1);

    let total_records = get_re_total_records()
        .captures(html)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(animes.len() as u32);

    Ok(SearchResult {
        animes,
        pagination: SearchPagination {
            current_page,
            total_pages,
            total_records,
        },
    })
}

/// Divide el bloque de resultados en objetos individuales y los parsea.
fn parse_anime_objects(block: &str) -> Vec<Anime> {
    let objects = iter_top_level_objects(block);
    let mut animes = Vec::with_capacity(objects.len());

    for obj in objects {
        match parse_single_anime(obj) {
            Some(anime) => animes.push(anime),
            None => {
                // No loguear a stderr para evitar corromper la pantalla TUI
                logger::log_warn(
                    "scraper::search",
                    &format!("no se pudo parsear objeto: {}...", truncate_chars(obj, 100)),
                );
            }
        }
    }

    animes
}

/// Extrae los campos de un objeto de anime individual.
///
/// El objeto tiene el formato JS literal (no JSON estricto):
/// ```
/// {id:"197",title:"One Piece",synopsis:"...",categoryId:1,slug:"one-piece",category:{...}}
/// ```
fn parse_single_anime(obj: &str) -> Option<Anime> {
    // — id —
    let id = get_re_field_id()
        .captures(obj)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())?;

    // — title —
    let title = get_re_field_title()
        .captures(obj)
        .and_then(|c| c.get(1))
        .map(|m| unescape_js_string(m.as_str()))?;

    // — slug: tomar el primero (del anime, no de la categoría) —
    let slug = get_re_field_slug()
        .captures(obj)
        .and_then(|c| c.get(1))
        .map(|m| unescape_js_string(m.as_str()))?;

    // — synopsis (puede estar ausente) —
    let synopsis = get_re_field_synopsis()
        .captures(obj)
        .and_then(|c| c.get(1))
        .map(|m| unescape_js_string(m.as_str()))
        .unwrap_or_default();

    // — categoryId —
    let category_id = get_re_field_category_id()
        .captures(obj)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(0);

    let category = AnimeCategory::from_id(category_id);

    Some(Anime {
        id,
        title,
        slug,
        synopsis,
        category,
        mal_id: None,         // Se puede extraer si se necesita
        episodes_count: None, // Se carga al navegar a la página del anime
        score: None,          // Se carga al navegar a la página del anime
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding_encodes_utf8_bytes_properly() {
        assert_eq!(urlencoding_simple("one piece"), "one+piece");
        assert_eq!(urlencoding_simple("ñ"), "%C3%B1");
        assert_eq!(urlencoding_simple("a_b-c.d~1"), "a_b-c.d~1");
    }

    #[test]
    fn parse_handles_escaped_quotes_and_braces_in_strings() {
        let fake_html = r#"results:[{id:"1",title:"Foo \"Bar\" Baz",synopsis:"Texto con },{ dentro",categoryId:5,slug:"foo-bar",category:{id:5,name:"ONA"}},{id:"2",title:"Segundo",synopsis:"Normal",categoryId:1,slug:"segundo",category:{id:1,name:"TV Anime"}}],totalPages:1,totalRecords:2"#;
        let res = parse_search_results(fake_html, 1).expect("parseok");
        assert_eq!(res.animes.len(), 2);
        assert_eq!(res.animes[0].title, "Foo \"Bar\" Baz");
        assert_eq!(res.animes[0].synopsis, "Texto con },{ dentro");
        assert_eq!(res.animes[0].category, AnimeCategory::Ona);
        assert_eq!(res.animes[1].slug, "segundo");
    }

    #[test]
    fn parse_live_captures_in_pruebas_if_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("pruebas");
        if !dir.exists() {
            return;
        }

        let naruto = std::fs::read_to_string(dir.join("catalogo_search_naruto.html")).unwrap();
        let res_n = parse_search_results(&naruto, 1).unwrap();
        assert_eq!(res_n.animes.len(), 19);
        assert!(res_n.animes.iter().any(|a| a.slug == "naruto"));

        let op = std::fs::read_to_string(dir.join("catalogo_search_one_piece.html")).unwrap();
        let res_op = parse_search_results(&op, 1).unwrap();
        assert_eq!(res_op.animes.len(), 20);
        assert!(res_op.animes.iter().any(|a| a.category == AnimeCategory::Ona));

        let lain = std::fs::read_to_string(dir.join("catalogo_search_lain.html")).unwrap();
        let res_l = parse_search_results(&lain, 1).unwrap();
        assert_eq!(res_l.animes.len(), 1);
        assert_eq!(res_l.animes[0].slug, "serial-experiments-lain");

        let rezero = std::fs::read_to_string(dir.join("catalogo_search_rezero.html")).unwrap();
        let res_r = parse_search_results(&rezero, 1).unwrap();
        assert!(!res_r.animes.is_empty());
    }
}

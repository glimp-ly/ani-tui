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

use anyhow::{Context, Result};
use regex::Regex;
use std::sync::OnceLock;

use crate::config::BASE_URL;
use crate::scraper::client::build_client;
use crate::structs::{Anime, AnimeCategory};

// ————————————————————————————————————————————————
// Patrones regex (compilados una sola vez al inicio)
// ————————————————————————————————————————————————

/// Extrae el bloque completo de resultados del HTML.
/// Captura todo lo que está entre `results:[` y el cierre del array `]`.
static RE_RESULTS_BLOCK: OnceLock<Regex> = OnceLock::new();

/// Extrae un objeto de resultado individual del bloque.
/// Los objetos tienen el formato {id:"...",title:"...",...}
static RE_SINGLE_RESULT: OnceLock<Regex> = OnceLock::new();

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

fn get_re_results_block() -> &'static Regex {
    RE_RESULTS_BLOCK.get_or_init(|| {
        // Captura el array de results completo (puede ser muy largo)
        // El lookahead ,total: marca el fin del array de results
        Regex::new(r"results:\[(\{.+?\})\],total:").expect("regex results_block inválida")
    })
}

fn get_re_single_result() -> &'static Regex {
    RE_SINGLE_RESULT.get_or_init(|| {
        // Cada resultado es un objeto JS separado por coma entre }{ 
        Regex::new(r"\{id:").expect("regex single_result inválida")
    })
}

fn get_re_field_id() -> &'static Regex {
    RE_FIELD_ID.get_or_init(|| {
        // id:"197" — ID como string entre comillas
        Regex::new(r#"^[^}]*?id:"(\d+)""#).expect("regex field_id inválida")
    })
}

fn get_re_field_title() -> &'static Regex {
    RE_FIELD_TITLE.get_or_init(|| {
        Regex::new(r#"title:"([^"]+)""#).expect("regex field_title inválida")
    })
}

fn get_re_field_slug() -> &'static Regex {
    RE_FIELD_SLUG.get_or_init(|| {
        // slug:"one-piece" — toma el primer slug encontrado en el objeto
        Regex::new(r#"slug:"([^"]+)""#).expect("regex field_slug inválida")
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
    let client = build_client().context("Error al inicializar cliente HTTP")?;

    // Construir URL de búsqueda con paginación
    let url = format!(
        "{}/catalogo?search={}&page={}",
        BASE_URL,
        urlencoding_simple(query),
        page
    );

    // Realizar petición HTTP
    let html = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Error al conectar con: {}", url))?
        .text()
        .await
        .context("Error al leer respuesta del servidor")?;

    // Parsear los resultados del HTML
    parse_search_results(&html, page)
}

/// Codificación URL simple para los términos de búsqueda.
/// Reemplaza espacios por '+' y codifica caracteres especiales básicos.
fn urlencoding_simple(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "+".to_string(),
            c if c.is_alphanumeric() || c == '-' || c == '_' => c.to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ————————————————————————————————————————————————
// Parseo del HTML
// ————————————————————————————————————————————————

/// Parsea los resultados de búsqueda del HTML del catálogo.
///
/// Estrategia:
/// 1. Localiza el bloque `results:[...]` en el HTML
/// 2. Divide el bloque en objetos individuales separados por `},{`
/// 3. Para cada objeto, extrae los campos con regex
/// 4. Extrae datos de paginación del bloque `pagination:{...}`
fn parse_search_results(html: &str, current_page: u32) -> Result<SearchResult> {
    // — Extraer el bloque de results —
    let re_block = get_re_results_block();
    let results_block = re_block
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        // Fallback: buscar de forma más laxa si la regex estricta falla
        .or_else(|| extract_results_block_fallback(html))
        .unwrap_or("");

    // Si no hay resultados, devolver lista vacía con paginación vacía
    if results_block.is_empty() {
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

/// Método de extracción alternativo para el bloque de results.
/// Se usa cuando el patrón principal no encuentra el bloque
/// (ej: el sitio cambió ligeramente la estructura).
fn extract_results_block_fallback(html: &str) -> Option<&str> {
    // Buscar la posición de `results:[` y extraer hasta `],total:`
    let start_marker = "results:[";
    let end_marker = "],total:";

    let start = html.find(start_marker)? + start_marker.len();
    let end = html[start..].find(end_marker).map(|i| start + i)?;

    Some(&html[start..end])
}

/// Divide el bloque de resultados en objetos individuales y los parsea.
///
/// El bloque tiene la forma: `{...},{...},{...}`
/// Se divide por `},{` respetando los objetos anidados.
fn parse_anime_objects(block: &str) -> Vec<Anime> {
    // Dividir por `},{` para separar los objetos de anime
    // Añadir marcadores para facilitar la separación
    let normalized = block.replace("},{", "}|||{");
    let objects: Vec<&str> = normalized.split("|||").collect();

    let mut animes = Vec::with_capacity(objects.len());

    for obj in &objects {
        match parse_single_anime(obj) {
            Some(anime) => animes.push(anime),
            None => {
                // Si falla el parseo de un objeto, continuar con el siguiente
                // (no fallar toda la búsqueda por un resultado corrupto)
                eprintln!("[scraper::search] advertencia: no se pudo parsear objeto: {}...",
                    &obj[..obj.len().min(100)]);
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
        .map(|m| m.as_str().to_string())?;

    // — slug: tomar el primero (del anime, no de la categoría) —
    let slug = get_re_field_slug()
        .captures(obj)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())?;

    // — synopsis (puede estar ausente) —
    let synopsis = get_re_field_synopsis()
        .captures(obj)
        .and_then(|c| c.get(1))
        .map(|m| {
            // Desescapar secuencias de escape básicas del HTML/JS
            m.as_str()
                .replace("\\n", "\n")
                .replace("\\\"", "\"")
                .replace("\\'", "'")
                .replace("\\\\", "\\")
        })
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
        mal_id: None,       // Se puede extraer si se necesita
        episodes_count: None, // Se carga al navegar a la página del anime
        score: None,          // Se carga al navegar a la página del anime
    })
}

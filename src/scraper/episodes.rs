// ============================================================
// src/scraper/episodes.rs — Obtención de episodios de un anime
// ============================================================
//
// Obtiene la lista de episodios disponibles para un anime dado,
// extrayendo los datos del HTML de la página /media/{slug}.
//
// El sitio embebe los datos en el HTML con el formato:
//   episodes:[{id:3433,number:1},{id:3434,number:2},...]
//
// Además se extraen metadatos del anime (episodesCount, score, etc.)
// que enriquecen la información mostrada en la TUI.
// ============================================================

use anyhow::Result;
use regex::Regex;
use std::sync::OnceLock;

use crate::config::BASE_URL;
use crate::logger;
use crate::scraper::client::fetch_html;
use crate::scraper::{extract_cover_from_html, find_balanced_content};
use crate::security;
use crate::structs::{Anime, AnimeCategory, Episode};

// ————————————————————————————————————————————————
// Patrones regex
// ————————————————————————————————————————————————

/// Extrae cada par id/number del bloque de episodios.
static RE_EPISODE_ENTRY: OnceLock<Regex> = OnceLock::new();

/// Extrae el total de episodios del anime.
static RE_EPISODES_COUNT: OnceLock<Regex> = OnceLock::new();

/// Extrae la puntuación del anime.
static RE_SCORE: OnceLock<Regex> = OnceLock::new();

/// Extrae el nombre de la categoría del anime.
static RE_CATEGORY_NAME: OnceLock<Regex> = OnceLock::new();

/// Extrae el ID de MAL.
static RE_MAL_ID: OnceLock<Regex> = OnceLock::new();

fn get_re_episode_entry() -> &'static Regex {
    RE_EPISODE_ENTRY.get_or_init(|| {
        // Captura {id:3433,number:1} tolerando espacios o campos adicionales
        Regex::new(r"\{[^}]*?\bid:(\d+)[^}]*?\bnumber:(\d+)[^}]*?\}")
            .expect("regex episode_entry inválida")
    })
}

fn get_re_episodes_count() -> &'static Regex {
    RE_EPISODES_COUNT.get_or_init(|| {
        Regex::new(r"episodesCount:(\d+)").expect("regex episodes_count inválida")
    })
}

fn get_re_score() -> &'static Regex {
    RE_SCORE.get_or_init(|| {
        Regex::new(r"score:([\d.]+)").expect("regex score inválida")
    })
}

fn get_re_category_name() -> &'static Regex {
    RE_CATEGORY_NAME.get_or_init(|| {
        // name:"TV Anime" dentro del objeto category
        Regex::new(r#"category:\{id:\d+,name:"([^"]+)""#).expect("regex category_name inválida")
    })
}

fn get_re_mal_id() -> &'static Regex {
    RE_MAL_ID.get_or_init(|| {
        Regex::new(r"malId:(\d+)").expect("regex mal_id inválida")
    })
}

// ————————————————————————————————————————————————
// Función principal
// ————————————————————————————————————————————————

/// Obtiene todos los episodios de un anime y enriquece sus metadatos.
///
/// # Parámetros
/// - `anime`: El anime para el que se quieren obtener episodios.
///   Se modifica en-place para agregar `episodes_count` y `score`.
///
/// # Retorna
/// Vector de episodios ordenados por número ascendente.
///
/// # Errores
/// - Error de red si el sitio no responde
/// - Error de parseo si la estructura HTML cambió
///
/// # Ejemplo
/// ```rust
/// let mut anime = search_anime("one piece", 1).await?.animes[0].clone();
/// let episodes = get_episodes(&mut anime).await?;
/// println!("{} tiene {} episodios", anime.title, episodes.len());
/// ```
pub async fn get_episodes(anime: &mut Anime) -> Result<Vec<Episode>> {
    let (episodes, metadata) = get_episodes_by_slug(&anime.slug).await?;

    // Enriquecer el anime con los metadatos encontrados
    if let Some(count) = metadata.episodes_count {
        anime.episodes_count = Some(count);
    }
    if let Some(score) = metadata.score {
        anime.score = Some(score);
    }
    if let Some(mal_id) = metadata.mal_id {
        anime.mal_id = Some(mal_id);
    }
    if matches!(anime.category, AnimeCategory::Unknown(_)) {
        if let Some(cat) = metadata.category {
            anime.category = cat;
        }
    }
    if let Some(cover_url) = metadata.cover_url {
        anime.cover_url = Some(cover_url);
    }

    Ok(episodes)
}

/// Obtiene episodios directamente por slug del anime.
/// Versión sin referencia mutable a `Anime`, útil para el modo API.
///
/// # Parámetros
/// - `slug`: Slug del anime (ej: "one-piece")
///
/// # Retorna
/// Tupla con los episodios y los metadatos del anime.
pub async fn get_episodes_by_slug(slug: &str) -> Result<(Vec<Episode>, AnimeMetadata)> {
    // SEGURIDAD: validar el slug antes de construir la URL
    security::validate_anime_slug(slug)
        .map_err(|e| anyhow::anyhow!("Slug inválido: {}", e))?;

    let url = format!("{}/media/{}", BASE_URL, slug);
    logger::log_net("GET", &url, "solicitando episodios...");

    let html = fetch_html(&url).await?;
    parse_episodes_from_html(&html, slug)
}

// ————————————————————————————————————————————————
// Metadatos del anime (extraídos junto con los episodios)
// ————————————————————————————————————————————————

/// Metadatos adicionales del anime extraídos de la página de episodios.
#[derive(Debug, Default)]
pub struct AnimeMetadata {
    /// Total de episodios según el contador del sitio
    pub episodes_count: Option<u32>,
    /// Puntuación del anime (0.0 - 10.0)
    pub score: Option<f32>,
    /// Categoría del anime
    pub category: Option<AnimeCategory>,
    /// ID en MyAnimeList
    pub mal_id: Option<u32>,
    /// URL de la imagen de portada del anime
    pub cover_url: Option<String>,
}

// ————————————————————————————————————————————————
// Parseo del HTML
// ————————————————————————————————————————————————

/// Parsea el HTML de la página de un anime para extraer episodios y metadatos.
pub fn parse_episodes_from_html(
    html: &str,
    anime_slug: &str,
) -> Result<(Vec<Episode>, AnimeMetadata)> {
    // — Extraer metadatos del anime —
    let metadata = extract_anime_metadata(html);

    // — Extraer bloque de episodios de forma balanceada —
    let episodes_block = find_balanced_content(html, "episodes:[", b'[', b']').unwrap_or("");

    if episodes_block.trim().is_empty() {
        // Si no se encontraron episodios, puede ser un anime sin episodios aún
        return Ok((vec![], metadata));
    }

    // — Parsear entradas individuales de episodio —
    let mut episodes: Vec<Episode> = get_re_episode_entry()
        .captures_iter(episodes_block)
        .filter_map(|caps| {
            let id = caps.get(1)?.as_str().parse::<u32>().ok()?;
            let number = caps.get(2)?.as_str().parse::<u32>().ok()?;
            Some(Episode {
                id,
                number,
                anime_slug: anime_slug.to_string(),
            })
        })
        .collect();

    // Ordenar por número ascendente (generalmente ya vienen ordenados,
    // pero se garantiza el orden correcto)
    episodes.sort_unstable_by_key(|e| e.number);

    Ok((episodes, metadata))
}

/// Extrae metadatos del anime del HTML.
fn extract_anime_metadata(html: &str) -> AnimeMetadata {
    let episodes_count = get_re_episodes_count()
        .captures(html)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok());

    let score = get_re_score()
        .captures(html)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<f32>().ok());

    let category = get_re_category_name()
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| AnimeCategory::from_name(m.as_str()));

    let mal_id = get_re_mal_id()
        .captures(html)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok());

    let cover_url = extract_cover_from_html(html);

    AnimeMetadata {
        episodes_count,
        score,
        category,
        mal_id,
        cover_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_episodes_fixture_sorts_and_extracts_metadata() {
        let html = r#"<img class="aspect-poster" src="https://cdn.animeav1.com/covers/42.jpg" alt="Demo Poster"/>category:{id:5,name:"ONA"},episodesCount:2,score:8.9,malId:42,episodes:[{id:20,number:2},{id:10,number:1}]"#;
        let (eps, meta) = parse_episodes_from_html(html, "demo-anime").unwrap();
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].number, 1);
        assert_eq!(eps[1].number, 2);
        assert_eq!(meta.episodes_count, Some(2));
        assert_eq!(meta.score, Some(8.9));
        assert_eq!(meta.mal_id, Some(42));
        assert_eq!(meta.category, Some(AnimeCategory::Ona));
        assert_eq!(
            meta.cover_url.as_deref(),
            Some("https://cdn.animeav1.com/covers/42.jpg")
        );
    }

    #[test]
    fn parse_live_episodes_in_pruebas_if_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("pruebas");
        if !dir.exists() {
            return;
        }

        let naruto_html = std::fs::read_to_string(dir.join("media_naruto.html")).unwrap();
        let (eps_n, meta_n) = parse_episodes_from_html(&naruto_html, "naruto").unwrap();
        assert_eq!(eps_n.len(), 220);
        assert_eq!(eps_n[0].number, 1);
        assert_eq!(eps_n[219].number, 220);
        assert!(meta_n.score.is_some());
        assert_eq!(
            meta_n.cover_url.as_deref(),
            Some("https://cdn.animeav1.com/covers/190.jpg")
        );

        let op_html = std::fs::read_to_string(dir.join("media_one_piece.html")).unwrap();
        let (eps_op, meta_op) = parse_episodes_from_html(&op_html, "one-piece").unwrap();
        assert!(eps_op.len() >= 1150, "esperados >=1150 episodios, got {}", eps_op.len());
        assert_eq!(eps_op[0].number, 1);
        assert!(eps_op.windows(2).all(|w| w[0].number <= w[1].number));
        assert!(meta_op.episodes_count.is_some());
        assert_eq!(
            meta_op.cover_url.as_deref(),
            Some("https://cdn.animeav1.com/covers/197.jpg")
        );

        let lain_html = std::fs::read_to_string(dir.join("media_serial_experiments_lain.html")).unwrap();
        let (eps_l, meta_l) = parse_episodes_from_html(&lain_html, "serial-experiments-lain").unwrap();
        assert_eq!(eps_l.len(), 13);
        assert_eq!(
            meta_l.cover_url.as_deref(),
            Some("https://cdn.animeav1.com/covers/1684.jpg")
        );

        let rezero_html = std::fs::read_to_string(dir.join("media_rezero4.html")).unwrap();
        let (eps_rz, meta_rz) = parse_episodes_from_html(&rezero_html, "rezero4").unwrap();
        assert!(!eps_rz.is_empty());
        assert_eq!(eps_rz[0].number, 1);
        assert_eq!(
            meta_rz.cover_url.as_deref(),
            Some("https://cdn.animeav1.com/covers/3968.jpg")
        );
    }
}

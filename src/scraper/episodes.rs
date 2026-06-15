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

use anyhow::{Context, Result};
use regex::Regex;
use std::sync::OnceLock;

use crate::config::BASE_URL;
use crate::scraper::client::build_client;
use crate::structs::{Anime, AnimeCategory, Episode};

// ————————————————————————————————————————————————
// Patrones regex
// ————————————————————————————————————————————————

/// Extrae el array de episodios del HTML.
/// Formato: episodes:[{id:N,number:M},{id:N2,number:M2},...]
static RE_EPISODES_BLOCK: OnceLock<Regex> = OnceLock::new();

/// Extrae cada par id/number del bloque de episodios.
static RE_EPISODE_ENTRY: OnceLock<Regex> = OnceLock::new();

/// Extrae el total de episodios del anime.
static RE_EPISODES_COUNT: OnceLock<Regex> = OnceLock::new();

/// Extrae la puntuación del anime.
static RE_SCORE: OnceLock<Regex> = OnceLock::new();

/// Extrae el slug del anime desde el HTML de la página.
static RE_SLUG: OnceLock<Regex> = OnceLock::new();

/// Extrae el nombre de la categoría del anime.
static RE_CATEGORY_NAME: OnceLock<Regex> = OnceLock::new();

/// Extrae el ID de MAL.
static RE_MAL_ID: OnceLock<Regex> = OnceLock::new();

fn get_re_episodes_block() -> &'static Regex {
    RE_EPISODES_BLOCK.get_or_init(|| {
        // Captura el contenido entre episodes:[ y el primer ] fuera del bloque
        // El bloque puede ser muy largo (One Piece tiene 1166 episodios)
        Regex::new(r"episodes:\[(\{[^]]+\})\]").expect("regex episodes_block inválida")
    })
}

fn get_re_episode_entry() -> &'static Regex {
    RE_EPISODE_ENTRY.get_or_init(|| {
        // Captura {id:3433,number:1} — enteros, sin comillas
        Regex::new(r"\{id:(\d+),number:(\d+)\}").expect("regex episode_entry inválida")
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

fn get_re_slug() -> &'static Regex {
    RE_SLUG.get_or_init(|| {
        // El primer slug encontrado es el del anime
        Regex::new(r#"slug:"([^"]+)""#).expect("regex slug inválida")
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
    let client = build_client().context("Error al inicializar cliente HTTP")?;

    let url = format!("{}/media/{}", BASE_URL, anime.slug);

    let html = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Error al conectar con: {}", url))?
        .text()
        .await
        .context("Error al leer respuesta del servidor")?;

    // Parsear episodios y metadatos del HTML
    let (episodes, metadata) = parse_episodes_from_html(&html, &anime.slug)?;

    // Enriquecer el anime con los metadatos encontrados
    if let Some(count) = metadata.episodes_count {
        anime.episodes_count = Some(count);
    }
    if let Some(score) = metadata.score {
        anime.score = Some(score);
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
    let client = build_client().context("Error al inicializar cliente HTTP")?;

    let url = format!("{}/media/{}", BASE_URL, slug);

    let html = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Error al conectar con: {}", url))?
        .text()
        .await
        .context("Error al leer respuesta del servidor")?;

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
}

// ————————————————————————————————————————————————
// Parseo del HTML
// ————————————————————————————————————————————————

/// Parsea el HTML de la página de un anime para extraer episodios y metadatos.
fn parse_episodes_from_html(
    html: &str,
    anime_slug: &str,
) -> Result<(Vec<Episode>, AnimeMetadata)> {
    // — Extraer metadatos del anime —
    let metadata = extract_anime_metadata(html);

    // — Extraer bloque de episodios —
    let episodes_block = get_re_episodes_block()
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .or_else(|| extract_episodes_block_fallback(html))
        .unwrap_or("");

    if episodes_block.is_empty() {
        // Si no se encontraron episodios, puede ser un error del sitio
        // o un anime sin episodios disponibles aún
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

    AnimeMetadata {
        episodes_count,
        score,
        category,
        mal_id,
    }
}

/// Método de extracción alternativo para el bloque de episodios.
/// Se usa cuando el bloque es demasiado largo para la regex estándar.
fn extract_episodes_block_fallback(html: &str) -> Option<&str> {
    let start_marker = "episodes:[";
    let start = html.find(start_marker)? + start_marker.len();

    // Encontrar el cierre del array contando corchetes
    let mut depth = 1i32;
    let mut end = start;
    let bytes = html[start..].as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }

    if end > start {
        Some(&html[start..end])
    } else {
        None
    }
}

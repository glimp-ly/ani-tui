// ============================================================
// src/scraper/sources.rs — Obtención de fuentes de video
// ============================================================
//
// Obtiene las fuentes de video (embeds) para un episodio específico,
// extrayendo los datos del HTML de la página /media/{slug}/{number}.
//
// El sitio embebe los datos de reproducción directamente en el HTML:
//   embeds:{
//     SUB:[{server:"HLS",url:"..."},{server:"Mega",url:"..."},...],
//     DUB:[{server:"HLS",url:"..."},...]   <- puede no existir
//   }
//
// No se necesita JavaScript, headless Chrome ni WebDriver.
// Los embeds incluyen servidores como HLS (m3u8), Mega, MP4Upload,
// UPNShare, TeraBox y PixelDrain.
// ============================================================

use anyhow::{Context, Result};
use regex::Regex;
use std::sync::OnceLock;

use crate::config::BASE_URL;
use crate::scraper::client::build_client;
use crate::structs::{AudioType, EpisodeSources, VideoSource};

// ————————————————————————————————————————————————
// Patrones regex
// ————————————————————————————————————————————————

/// Extrae el bloque completo de embeds del HTML.
/// El bloque termina antes de `,downloads:` o `},uses:`.
static RE_EMBEDS_BLOCK: OnceLock<Regex> = OnceLock::new();

/// Extrae el sub-bloque SUB del bloque de embeds.
static RE_SUB_BLOCK: OnceLock<Regex> = OnceLock::new();

/// Extrae el sub-bloque DUB del bloque de embeds.
static RE_DUB_BLOCK: OnceLock<Regex> = OnceLock::new();

/// Extrae pares {server:"X",url:"Y"} de un bloque de fuentes.
static RE_SOURCE_ENTRY: OnceLock<Regex> = OnceLock::new();

fn get_re_embeds_block() -> &'static Regex {
    RE_EMBEDS_BLOCK.get_or_init(|| {
        // Captura el contenido del objeto embeds hasta el inicio de downloads
        // o hasta el cierre del objeto padre
        Regex::new(r"embeds:\{(.*?)\},downloads:").expect("regex embeds_block inválida")
    })
}

fn get_re_sub_block() -> &'static Regex {
    RE_SUB_BLOCK.get_or_init(|| {
        // SUB:[{...},{...},...] hasta el siguiente campo (DUB o cierre)
        Regex::new(r"SUB:\[([^\]]+)\]").expect("regex sub_block inválida")
    })
}

fn get_re_dub_block() -> &'static Regex {
    RE_DUB_BLOCK.get_or_init(|| {
        Regex::new(r"DUB:\[([^\]]+)\]").expect("regex dub_block inválida")
    })
}

fn get_re_source_entry() -> &'static Regex {
    RE_SOURCE_ENTRY.get_or_init(|| {
        // Captura {server:"HLS",url:"https://..."} con URL posiblemente larga
        Regex::new(r#"\{server:"([^"]+)",url:"([^"]+)"\}"#)
            .expect("regex source_entry inválida")
    })
}

// ————————————————————————————————————————————————
// Función principal
// ————————————————————————————————————————————————

/// Obtiene todas las fuentes de video disponibles para un episodio.
///
/// # Parámetros
/// - `anime_slug`: Slug del anime (ej: "one-piece")
/// - `episode_number`: Número del episodio (ej: 1)
///
/// # Retorna
/// `EpisodeSources` con las fuentes SUB y DUB disponibles.
/// El campo DUB puede estar vacío si el anime no tiene doblaje.
///
/// # Prioridad para reproducción
/// Para usar con `mpv`, preferir en este orden:
/// 1. `HLS` (m3u8 directo, el más compatible)
/// 2. `PDrain` (PixelDrain, acceso directo)
/// 3. Fallback: abrir cualquier otra URL en el navegador
///
/// # Ejemplo
/// ```rust
/// let sources = get_video_sources("serial-experiments-lain", 1).await?;
/// println!("Fuentes SUB: {}", sources.sub.len());
/// println!("Fuentes DUB: {}", sources.dub.len());
///
/// if let Some(hls) = sources.best_for_mpv(&AudioType::Sub) {
///     println!("URL para mpv: {}", hls.url);
/// }
/// ```
pub async fn get_video_sources(
    anime_slug: &str,
    episode_number: u32,
) -> Result<EpisodeSources> {
    let client = build_client().context("Error al inicializar cliente HTTP")?;

    let url = format!("{}/media/{}/{}", BASE_URL, anime_slug, episode_number);

    let html = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Error al conectar con: {}", url))?
        .text()
        .await
        .context("Error al leer respuesta del servidor")?;

    parse_sources_from_html(&html)
        .with_context(|| format!("Error al parsear fuentes del episodio {}", episode_number))
}

// ————————————————————————————————————————————————
// Parseo del HTML
// ————————————————————————————————————————————————

/// Parsea las fuentes de video del HTML de la página de un episodio.
///
/// Estrategia:
/// 1. Localiza el bloque `embeds:{...}` en el HTML
/// 2. Extrae los sub-bloques `SUB:[...]` y `DUB:[...]`
/// 3. Para cada bloque, extrae pares {server,url}
/// 4. Construye `VideoSource` con tipo de audio correcto
pub fn parse_sources_from_html(html: &str) -> Result<EpisodeSources> {
    // — Extraer bloque de embeds —
    let embeds_content = get_re_embeds_block()
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .or_else(|| extract_embeds_block_fallback(html))
        .unwrap_or("");

    if embeds_content.is_empty() {
        // El episodio podría no tener fuentes disponibles aún
        return Ok(EpisodeSources::default());
    }

    // — Extraer fuentes SUB —
    let sub_sources = extract_sources_from_block(embeds_content, AudioType::Sub);

    // — Extraer fuentes DUB (puede no existir) —
    let dub_sources = extract_sources_from_block(embeds_content, AudioType::Dub);

    Ok(EpisodeSources {
        sub: sub_sources,
        dub: dub_sources,
    })
}

/// Extrae las fuentes de un bloque de audio (SUB o DUB).
fn extract_sources_from_block(embeds_content: &str, audio: AudioType) -> Vec<VideoSource> {
    // Seleccionar regex según tipo de audio
    let re_block = match audio {
        AudioType::Sub => get_re_sub_block(),
        AudioType::Dub => get_re_dub_block(),
    };

    // Extraer el bloque del tipo de audio
    let audio_block = match re_block
        .captures(embeds_content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
    {
        Some(b) => b,
        None => return vec![],
    };

    // Parsear cada fuente individual
    get_re_source_entry()
        .captures_iter(audio_block)
        .filter_map(|caps| {
            let server = caps.get(1)?.as_str().to_string();
            let url = caps.get(2)?.as_str().to_string();

            // Validar que la URL sea válida
            if url.is_empty() || !url.starts_with("http") {
                return None;
            }

            Some(VideoSource {
                server,
                url,
                audio: audio.clone(),
                quality: None, // El sitio no especifica calidad en los embeds
            })
        })
        .collect()
}

/// Método alternativo de extracción del bloque de embeds.
/// Se usa cuando el patrón principal (que depende de `,downloads:`) falla,
/// por ejemplo si el episodio no tiene enlaces de descarga.
fn extract_embeds_block_fallback(html: &str) -> Option<&str> {
    let start_marker = "embeds:{";
    let start = html.find(start_marker)? + start_marker.len() - 1; // incluir el {

    // Contar llaves para encontrar el cierre correcto
    let mut depth = 0i32;
    let mut end = start;
    let bytes = html[start..].as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end > start {
        // Devolver el contenido interior (sin las llaves externas)
        Some(&html[start + 1..end - 1])
    } else {
        None
    }
}

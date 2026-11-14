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
use crate::logger;
use crate::scraper::client::fetch_html;
use crate::scraper::{find_balanced_content, iter_top_level_objects, unescape_js_string};
use crate::security;
use crate::structs::{AudioType, EpisodeSources, VideoSource};

// ————————————————————————————————————————————————
// Patrones regex
// ————————————————————————————————————————————————

/// Extrae el campo `server` de una fuente.
static RE_SOURCE_SERVER: OnceLock<Regex> = OnceLock::new();

/// Extrae el campo `url` de una fuente.
static RE_SOURCE_URL: OnceLock<Regex> = OnceLock::new();

fn get_re_source_server() -> &'static Regex {
    RE_SOURCE_SERVER.get_or_init(|| {
        Regex::new(r#"server\s*:\s*"((?:[^"\\]|\\.)*)""#)
            .expect("regex source_server inválida")
    })
}

fn get_re_source_url() -> &'static Regex {
    RE_SOURCE_URL.get_or_init(|| {
        Regex::new(r#"url\s*:\s*"((?:[^"\\]|\\.)*)""#)
            .expect("regex source_url inválida")
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
/// 1. `HLS` (m3u8 directo con headers de Zilla)
/// 2. `MP4Upload` (MP4 directo del embed)
/// 3. `YourUpload` (MP4 directo del embed)
/// 4. `PDrain` (PixelDrain, descarga directa)
/// 5. Fallback: abrir cualquier otra URL en el navegador
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
    // SEGURIDAD: validar slug y número de episodio antes de construir la URL
    security::validate_anime_slug(anime_slug)
        .map_err(|e| anyhow::anyhow!("Slug inválido: {}", e))?;
    security::validate_episode_number(episode_number)
        .map_err(|e| anyhow::anyhow!("Número de episodio inválido: {}", e))?;

    let url = format!("{}/media/{}/{}", BASE_URL, anime_slug, episode_number);
    logger::log_net("GET", &url, "solicitando fuentes de video...");

    let html = fetch_html(&url).await?;

    parse_sources_from_html(&html)
        .with_context(|| format!("Error al parsear fuentes del episodio {}", episode_number))
}

// ————————————————————————————————————————————————
// Parseo del HTML
// ————————————————————————————————————————————————

/// Parsea las fuentes de video del HTML de la página de un episodio.
///
/// Estrategia:
/// 1. Localiza el bloque `embeds:{...}` con conteo balanceado respetando comillas
/// 2. Extrae los sub-bloques `SUB:[...]` y `DUB:[...]`
/// 3. Para cada objeto, extrae los campos `server` y `url`
/// 4. Construye `VideoSource` con tipo de audio correcto
pub fn parse_sources_from_html(html: &str) -> Result<EpisodeSources> {
    let embeds_content = find_balanced_content(html, "embeds:{", b'{', b'}').unwrap_or("");

    if embeds_content.is_empty() {
        // El episodio podría no tener fuentes disponibles aún
        return Ok(EpisodeSources::default());
    }

    let sub_sources = extract_sources_from_block(embeds_content, AudioType::Sub);
    let dub_sources = extract_sources_from_block(embeds_content, AudioType::Dub);

    Ok(EpisodeSources {
        sub: sub_sources,
        dub: dub_sources,
    })
}

/// Extrae las fuentes de un bloque de audio (SUB o DUB).
fn extract_sources_from_block(embeds_content: &str, audio: AudioType) -> Vec<VideoSource> {
    let marker = match audio {
        AudioType::Sub => "SUB:[",
        AudioType::Dub => "DUB:[",
    };

    let Some(audio_block) = find_balanced_content(embeds_content, marker, b'[', b']') else {
        return vec![];
    };

    iter_top_level_objects(audio_block)
        .into_iter()
        .filter_map(|obj| {
            let server = get_re_source_server()
                .captures(obj)
                .and_then(|c| c.get(1))
                .map(|m| unescape_js_string(m.as_str()))?;
            let url = get_re_source_url()
                .captures(obj)
                .and_then(|c| c.get(1))
                .map(|m| unescape_js_string(m.as_str()))?;

            // Validar que la URL sea HTTP/HTTPS
            if url.is_empty() || (!url.starts_with("https://") && !url.starts_with("http://")) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_live_sources_in_pruebas_if_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("pruebas");
        if !dir.exists() {
            return;
        }

        for (name, file) in [
            ("naruto", "media_naruto_ep1.html"),
            ("one_piece", "media_one_piece_ep1.html"),
            ("lain", "media_serial_lain_ep1.html"),
            ("rezero4", "media_rezero4_ep1.html"),
        ] {
            let html = std::fs::read_to_string(dir.join(file)).unwrap();
            let sources = parse_sources_from_html(&html)
                .unwrap_or_else(|e| panic!("falló parseo {}: {}", name, e));
            assert!(!sources.sub.is_empty(), "{}: sin fuentes SUB", name);
            assert!(
                sources.sub.iter().any(|s| s.server == "HLS"),
                "{}: sin servidor HLS",
                name
            );
            assert!(
                sources.best_for_mpv(&AudioType::Sub).is_some(),
                "{}: sin fuente compatible con mpv",
                name
            );
        }

        let naruto_html = std::fs::read_to_string(dir.join("media_naruto_ep1.html")).unwrap();
        let naruto = parse_sources_from_html(&naruto_html).unwrap();
        assert!(naruto.has_dub(), "naruto ep1 debería tener DUB");
        assert!(
            !naruto.sub.iter().any(|s| s.server == "1Fichier"),
            "downloads no debe mezclarse en embeds"
        );
        assert!(naruto.sub.iter().any(|s| s.server == "YourUpload"));
        assert!(naruto.sub.iter().any(|s| s.server == "MP4Upload"));
    }

    #[test]
    fn parse_handles_brackets_inside_url_and_extra_fields() {
        let html = r#"embeds:{SUB:[{quality:"1080p",server:"HLS",url:"https://player.zilla-networks.com/play/0123456789abcdef0123456789abcdef?a=[1]"}],DUB:[]},downloads:{}"#;
        let sources = parse_sources_from_html(html).unwrap();
        assert_eq!(sources.sub.len(), 1);
        assert_eq!(sources.sub[0].server, "HLS");
        assert!(sources.sub[0].url.contains("a=[1]"));
        assert!(sources.dub.is_empty());
    }
}

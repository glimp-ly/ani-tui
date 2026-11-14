// ============================================================
// src/resolver.rs — Resolución de URLs de video reproducibles
// ============================================================
//
// Convierte las URLs de embed/player a URLs directas reproducibles
// con mpv. Cada proveedor tiene su propia lógica de extracción.
//
// Proveedores soportados y su estrategia:
//
//   ZILLA NETWORKS (HLS):
//     URL embed:  https://player.zilla-networks.com/play/{32-char-id}
//     URL m3u8:   https://player.zilla-networks.com/m3u8/{32-char-id}
//     Estrategia: extraer el ID del path y construir la URL m3u8 directa.
//     El stream requiere el header Referer para autenticarse.
//
//   PIXELDRAIN:
//     URL embed:  https://pixeldrain.com/u/{file-id}?embed
//     URL directa: https://pixeldrain.com/api/file/{file-id}?download
//     Estrategia: extraer el file-id y construir URL de descarga directa.
//
//   MEGA:
//     No resoluble sin JavaScript (cifrado del lado del cliente).
//     → Siempre abrir en navegador.
//
//   MP4UPLOAD:
//     URL embed: https://www.mp4upload.com/embed-{file-id}.html
//     Estrategia: hacer scraping del HTML del embed para encontrar
//                 el src del tag <source> dentro del <video>.
//
//   UPNSHARE (animeav1.uns.bio):
//     No resoluble directamente.
//     → Abrir en navegador.
// ============================================================

use anyhow::Result;
use regex::Regex;
use std::sync::OnceLock;

use crate::logger;
use crate::scraper::client::get_client;
use crate::structs::truncate_chars;

// ————————————————————————————————————————————————
// Tipos de resultado de resolución
// ————————————————————————————————————————————————

/// Resultado de intentar resolver una URL de embed.
#[derive(Debug, Clone)]
pub enum ResolvedUrl {
    /// URL directa de stream (m3u8, mp4) reproducible con mpv.
    /// Incluye Referer y cabeceras HTTP adicionales cuando el CDN las exige.
    DirectStream {
        url: String,
        /// Header Referer necesario para la petición (si aplica)
        referer: Option<String>,
        /// Cabeceras HTTP adicionales requeridas (ej: Origin, Sec-Fetch-*)
        headers: Vec<(String, String)>,
        /// Tipo de contenido aproximado
        content_type: StreamType,
    },
    /// No se pudo resolver — usar el navegador.
    FallbackBrowser {
        url: String,
        reason: String,
    },
}

/// Tipo de stream de video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamType {
    /// HLS playlist (application/x-mpegURL)
    Hls,
    /// MP4 directo
    Mp4,
}

impl StreamType {
    pub fn display(&self) -> &str {
        match self {
            StreamType::Hls => "HLS (m3u8)",
            StreamType::Mp4 => "MP4 directo",
        }
    }
}

// ————————————————————————————————————————————————
// Helpers de validación URL y fetch HTTP compartido
// ————————————————————————————————————————————————

fn is_http_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn extract_host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = host_port.split(':').next().unwrap_or(host_port);
    (!host.is_empty()).then_some(host)
}

fn host_matches(url: &str, domain: &str) -> bool {
    extract_host(url)
        .map(|h| {
            let h = h.to_ascii_lowercase();
            h == domain || h.ends_with(&format!(".{}", domain))
        })
        .unwrap_or(false)
}

fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else {
        ("http", url.strip_prefix("http://")?)
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    (!authority.is_empty()).then(|| format!("{}://{}", scheme, authority))
}

async fn fetch_embed_html(tag: &str, embed_url: &str, referer: &str) -> Result<String, String> {
    logger::log_net("GET", embed_url, &format!("iniciando fetch {}...", tag));

    let client = get_client().map_err(|e| {
        logger::log_error(tag, &format!("Error cliente HTTP: {}", e));
        format!("Error al obtener cliente HTTP: {}", e)
    })?;

    let resp = client
        .get(embed_url)
        .header("Referer", referer)
        .send()
        .await
        .map_err(|e| {
            logger::log_error(tag, &format!("Error fetch: {}", e));
            format!("Error de red al acceder a {}: {}", tag, e)
        })?;

    let status = resp.status();
    logger::log_net("GET", embed_url, &status.to_string());

    let resp = resp.error_for_status().map_err(|e| {
        logger::log_warn(tag, &format!("HTTP de error ({}): {}", status, e));
        format!("HTTP {} al consultar embed", status)
    })?;

    resp.text().await.map_err(|e| {
        logger::log_error(tag, &format!("Error leyendo respuesta: {}", e));
        format!("Error al leer HTML del embed: {}", e)
    })
}

// ————————————————————————————————————————————————
// Función principal de resolución
// ————————————————————————————————————————————————

/// Intenta resolver la URL de embed/player a una URL directa reproducible.
///
/// # Parámetros
/// - `server`: Nombre del servidor (ej: "HLS", "PDrain", "MP4Upload", "YourUpload")
/// - `embed_url`: URL del embed tal como viene en el HTML del sitio
///
/// # Retorna
/// `ResolvedUrl::DirectStream` si se pudo extraer una URL directa,
/// `ResolvedUrl::FallbackBrowser` si hay que abrir en el navegador.
pub async fn resolve_url(server: &str, embed_url: &str) -> ResolvedUrl {
    logger::log_info("resolver", &format!(
        "Resolviendo servidor={} url={}",
        server,
        truncate_chars(embed_url, 60)
    ));

    // SEGURIDAD: Validar que la URL sea HTTP/HTTPS antes de procesarla
    if !is_http_url(embed_url) {
        logger::log_security("resolver", &format!(
            "URL rechazada: no es http/https — servidor={}", server
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "URL con esquema no permitido (solo http/https)".to_string(),
        };
    }

    match server {
        "HLS" => resolve_zilla_hls(embed_url),
        "PDrain" => resolve_pixeldrain(embed_url),
        "MP4Upload" => resolve_mp4upload(embed_url).await,
        "YourUpload" => resolve_yourupload(embed_url).await,
        other => {
            logger::log_info("resolver", &format!(
                "Servidor '{}' sin resolución directa → navegador", other
            ));
            ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason: format!("Servidor '{}' no tiene resolución directa implementada", other),
            }
        }
    }
}

// ————————————————————————————————————————————————
// Zilla Networks (HLS)
// ————————————————————————————————————————————————

/// Regex para extraer el ID de 32 caracteres hex de la URL del player.
/// Ejemplo: /play/7601658844858539e438dceee32e7924
static RE_ZILLA_ID: OnceLock<Regex> = OnceLock::new();

fn get_re_zilla_id() -> &'static Regex {
    RE_ZILLA_ID.get_or_init(|| {
        Regex::new(r"(?i)/play/([0-9a-f]{32})").expect("regex zilla_id inválida")
    })
}

fn build_zilla_stream(m3u8_url: String, referer: String, origin: String) -> ResolvedUrl {
    ResolvedUrl::DirectStream {
        url: m3u8_url,
        referer: Some(referer),
        // Cloudflare WAF de Zilla exige Sec-Fetch-Site: same-origin en /segs/*/*.html
        headers: vec![
            ("Origin".to_string(), origin),
            ("Sec-Fetch-Dest".to_string(), "empty".to_string()),
            ("Sec-Fetch-Mode".to_string(), "cors".to_string()),
            ("Sec-Fetch-Site".to_string(), "same-origin".to_string()),
        ],
        content_type: StreamType::Hls,
    }
}

/// Resuelve la URL del player de Zilla Networks a su m3u8 directo.
///
/// La lógica se descubrió analizando el JS del player (assets/index-DOYntJxP.js):
///   file: "https://player.zilla-networks.com/m3u8/" + id_32chars
///
/// El m3u8 sirve segmentos con extensión .html (técnica anti-scraping)
/// pero mpv los maneja correctamente como contenido de video.
///
/// IMPORTANTE: Se necesita el header Referer para que el CDN Cloudflare
/// no bloquee la petición (verifica que venga del player legítimo).
pub fn resolve_zilla_hls(embed_url: &str) -> ResolvedUrl {
    let origin = origin_of(embed_url)
        .unwrap_or_else(|| "https://player.zilla-networks.com".to_string());

    // Extraer el ID de 32 chars del path
    let id = match get_re_zilla_id()
        .captures(embed_url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
    {
        Some(id) => id,
        None => {
            // Intentar el formato alternativo (si la URL ya es un m3u8)
            if embed_url.contains("/m3u8/") {
                logger::log_info("resolver::zilla", "URL ya es m3u8 directo");
                return build_zilla_stream(
                    embed_url.to_string(),
                    embed_url.replace("/m3u8/", "/play/"),
                    origin,
                );
            }

            logger::log_warn("resolver::zilla", &format!(
                "No se pudo extraer ID de la URL del player: {}", embed_url
            ));
            return ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason: "No se pudo extraer el ID del player de Zilla Networks".to_string(),
            };
        }
    };

    // SEGURIDAD: Validar que el ID sea exactamente 32 caracteres hexadecimales
    if id.len() != 32 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        logger::log_security("resolver::zilla", &format!(
            "ID rechazado: formato inválido (len={}, id={})", id.len(), truncate_chars(&id, 10)
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "ID del player con formato inválido (posible ataque de path traversal)".to_string(),
        };
    }

    // Construir la URL m3u8 y Referer preservando el dominio original del embed
    let m3u8_url = format!("{}/m3u8/{}", origin, id);
    let referer = format!("{}/play/{}", origin, id);

    logger::log_info("resolver::zilla", &format!(
        "URL m3u8 resuelta para ID {} — tipo: HLS", truncate_chars(&id, 8)
    ));

    build_zilla_stream(m3u8_url, referer, origin)
}

// ————————————————————————————————————————————————
// PixelDrain
// ————————————————————————————————————————————————

/// Regex para extraer el file ID de PixelDrain.
/// Ejemplo: https://pixeldrain.com/u/MJzY7Ab1?embed
static RE_PIXELDRAIN_ID: OnceLock<Regex> = OnceLock::new();

fn get_re_pixeldrain_id() -> &'static Regex {
    RE_PIXELDRAIN_ID.get_or_init(|| {
        Regex::new(r"pixeldrain\.(?:com|dev)/u/([A-Za-z0-9]+)").expect("regex pixeldrain_id inválida")
    })
}

/// Resuelve la URL de embed de PixelDrain a la URL de descarga directa.
fn resolve_pixeldrain(embed_url: &str) -> ResolvedUrl {
    if !host_matches(embed_url, "pixeldrain.com") && !host_matches(embed_url, "pixeldrain.dev") {
        logger::log_security("resolver::pixeldrain", "URL no pertenece a pixeldrain");
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "URL no es de pixeldrain".to_string(),
        };
    }

    let file_id = match get_re_pixeldrain_id().captures(embed_url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
    {
        Some(id) => id,
        None => {
            logger::log_warn("resolver::pixeldrain", "No se pudo extraer file ID");
            return ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason: "No se pudo extraer el file ID de PixelDrain".to_string(),
            };
        }
    };

    // SEGURIDAD: Validar el file ID (solo alfanumérico)
    if file_id.len() > 20 || !file_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        logger::log_security("resolver::pixeldrain", &format!(
            "File ID rechazado: formato inválido ({})", truncate_chars(&file_id, 10)
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "File ID de PixelDrain con formato inválido".to_string(),
        };
    }

    let origin = origin_of(embed_url).unwrap_or_else(|| "https://pixeldrain.com".to_string());
    let direct_url = format!("{}/api/file/{}", origin, file_id);
    logger::log_info("resolver::pixeldrain", &format!(
        "URL directa resuelta para ID {}", file_id
    ));

    ResolvedUrl::DirectStream {
        url: direct_url,
        referer: None,
        headers: vec![],
        content_type: StreamType::Mp4,
    }
}

// ————————————————————————————————————————————————
// MP4Upload
// ————————————————————————————————————————————————

/// Regex para extraer `src: "https://....mp4"` del script embebido en MP4Upload.
static RE_MP4UPLOAD_SRC: OnceLock<Regex> = OnceLock::new();

/// Fallback para HTML con etiqueta `<source src="...mp4">`.
static RE_MP4UPLOAD_TAG: OnceLock<Regex> = OnceLock::new();

fn get_re_mp4upload_src() -> &'static Regex {
    RE_MP4UPLOAD_SRC.get_or_init(|| {
        Regex::new(r#"(?i)\bsrc\s*:\s*["'](https?://[^"']+\.mp4(?:[?#][^"']*)?)["']"#)
            .expect("regex mp4upload_src inválida")
    })
}

fn get_re_mp4upload_tag() -> &'static Regex {
    RE_MP4UPLOAD_TAG.get_or_init(|| {
        Regex::new(r#"(?i)<source\s+[^>]*\bsrc\s*=\s*["'](https?://[^"']+\.mp4(?:[?#][^"']*)?)["']"#)
            .expect("regex mp4upload_tag inválida")
    })
}

/// Extrae la URL directa `.mp4` del HTML de MP4Upload.
pub fn extract_mp4upload_url(html: &str) -> Option<String> {
    get_re_mp4upload_src()
        .captures(html)
        .or_else(|| get_re_mp4upload_tag().captures(html))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .filter(|u| is_http_url(u))
}

async fn resolve_mp4upload(embed_url: &str) -> ResolvedUrl {
    // SEGURIDAD: Verificar dominio exacto o subdominio de mp4upload.com
    if !host_matches(embed_url, "mp4upload.com") {
        logger::log_security("resolver::mp4upload", &format!(
            "URL rechazada: no es de mp4upload.com ({})", truncate_chars(embed_url, 40)
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "URL no es de mp4upload.com".to_string(),
        };
    }

    let html = match fetch_embed_html("resolver::mp4upload", embed_url, "https://animeav1.com/").await {
        Ok(text) => text,
        Err(reason) => {
            return ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason,
            };
        }
    };

    match extract_mp4upload_url(&html) {
        Some(mp4_url) => {
            logger::log_info("resolver::mp4upload", "URL MP4 directa extraída correctamente");
            ResolvedUrl::DirectStream {
                url: mp4_url,
                referer: Some("https://www.mp4upload.com/".to_string()),
                headers: vec![],
                content_type: StreamType::Mp4,
            }
        }
        None => {
            logger::log_warn("resolver::mp4upload", "No se encontró src del MP4 en el HTML");
            ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason: "No se encontró la URL del MP4 en el HTML del embed".to_string(),
            }
        }
    }
}

// ————————————————————————————————————————————————
// YourUpload
// ————————————————————————————————————————————————

/// Regex para extraer `file: 'https://.../video.mp4'` del player JWPlayer de YourUpload.
static RE_YOURUPLOAD_FILE: OnceLock<Regex> = OnceLock::new();

/// Fallback para la etiqueta `<meta property="og:video" content="...">`.
static RE_YOURUPLOAD_OG: OnceLock<Regex> = OnceLock::new();

fn get_re_yourupload_file() -> &'static Regex {
    RE_YOURUPLOAD_FILE.get_or_init(|| {
        Regex::new(r#"(?i)\bfile\s*:\s*["'](https?://[^"']+)["']"#)
            .expect("regex yourupload_file inválida")
    })
}

fn get_re_yourupload_og() -> &'static Regex {
    RE_YOURUPLOAD_OG.get_or_init(|| {
        Regex::new(r#"(?i)<meta\s+[^>]*property=["']og:video["'][^>]*content=["'](https?://[^"']+)["']"#)
            .expect("regex yourupload_og inválida")
    })
}

/// Extrae la URL directa del video desde el HTML de embed de YourUpload.
pub fn extract_yourupload_url(html: &str) -> Option<String> {
    get_re_yourupload_file()
        .captures(html)
        .or_else(|| get_re_yourupload_og().captures(html))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .filter(|u| is_http_url(u))
}

async fn resolve_yourupload(embed_url: &str) -> ResolvedUrl {
    if !host_matches(embed_url, "yourupload.com") {
        logger::log_security("resolver::yourupload", &format!(
            "URL rechazada: no es de yourupload.com ({})", truncate_chars(embed_url, 40)
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "URL no es de yourupload.com".to_string(),
        };
    }

    let html = match fetch_embed_html("resolver::yourupload", embed_url, "https://animeav1.com/").await {
        Ok(text) => text,
        Err(reason) => {
            return ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason,
            };
        }
    };

    match extract_yourupload_url(&html) {
        Some(mp4_url) => {
            logger::log_info("resolver::yourupload", "URL MP4 directa extraída correctamente");
            ResolvedUrl::DirectStream {
                url: mp4_url,
                referer: Some("https://www.yourupload.com/".to_string()),
                headers: vec![],
                content_type: StreamType::Mp4,
            }
        }
        None => {
            logger::log_warn("resolver::yourupload", "No se encontró URL del video en el HTML");
            ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason: "No se encontró la URL del video en YourUpload".to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zilla_resolver_adds_cloudflare_same_origin_headers() {
        let url = "https://player.zilla-networks.com/play/7601658844858539e438dceee32e7924";
        match resolve_zilla_hls(url) {
            ResolvedUrl::DirectStream {
                url: m3u8,
                referer,
                headers,
                content_type,
            } => {
                assert_eq!(
                    m3u8,
                    "https://player.zilla-networks.com/m3u8/7601658844858539e438dceee32e7924"
                );
                assert_eq!(referer.as_deref(), Some(url));
                assert_eq!(content_type, StreamType::Hls);
                assert!(headers.contains(&("Sec-Fetch-Site".into(), "same-origin".into())));
                assert!(headers.contains(&(
                    "Origin".into(),
                    "https://player.zilla-networks.com".into()
                )));
            }
            ResolvedUrl::FallbackBrowser { reason, .. } => panic!("esperado DirectStream: {}", reason),
        }
    }

    #[test]
    fn pixeldrain_validates_host_and_resolves_api_url() {
        match resolve_pixeldrain("https://pixeldrain.com/u/MJzY7Ab1?embed") {
            ResolvedUrl::DirectStream { url, .. } => {
                assert_eq!(url, "https://pixeldrain.com/api/file/MJzY7Ab1");
            }
            _ => panic!("esperado DirectStream"),
        }

        match resolve_pixeldrain("https://evil.com/pixeldrain.com/u/MJzY7Ab1") {
            ResolvedUrl::FallbackBrowser { .. } => {}
            _ => panic!("spoofed host debe rechazarse"),
        }
    }

    #[test]
    fn host_matching_prevents_spoofing() {
        assert!(host_matches("https://www.mp4upload.com/embed-abc.html", "mp4upload.com"));
        assert!(host_matches("https://mp4upload.com/embed-abc.html", "mp4upload.com"));
        assert!(!host_matches("https://evil.com/mp4upload.com", "mp4upload.com"));
        assert!(!host_matches("https://notyourupload.com/embed/123", "yourupload.com"));
    }

    #[test]
    fn extracts_mp4upload_from_html_and_live_capture_if_present() {
        let decoy = r#"src: "https://www.mp4upload.com/player/skins/nuevo.css", src: "https://a4.mp4upload.com:183/d/abc/video.mp4""#;
        assert_eq!(
            extract_mp4upload_url(decoy).as_deref(),
            Some("https://a4.mp4upload.com:183/d/abc/video.mp4")
        );

        let html_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("pruebas/mp4upload_embed.html");
        if html_path.exists() {
            let html = std::fs::read_to_string(html_path).unwrap();
            let url = extract_mp4upload_url(&html).expect("debe extraer URL mp4upload");
            assert_eq!(
                url,
                "https://a4.mp4upload.com:183/d/xkx2ce5vz3b4quuolwsbgjkiccf3rljyvzj3fakd3eekykun3zqcagw4m3jrxh7wvsrqz4i7/video.mp4"
            );
        }
    }

    #[test]
    fn extracts_yourupload_from_html_and_live_capture_if_present() {
        let inline = r#"<meta property="og:video" content="https://vidcache.net/og/video.mp4"><script>jwplayer().setup({ file: 'https://vidcache.net/live/video.mp4' });</script>"#;
        assert_eq!(
            extract_yourupload_url(inline).as_deref(),
            Some("https://vidcache.net/live/video.mp4")
        );

        let og_only = r#"<meta property="og:video" content="https://vidcache.net/og/video.mp4">"#;
        assert_eq!(
            extract_yourupload_url(og_only).as_deref(),
            Some("https://vidcache.net/og/video.mp4")
        );

        let html_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("pruebas/yourupload_embed.html");
        if html_path.exists() {
            let html = std::fs::read_to_string(html_path).unwrap();
            let url = extract_yourupload_url(&html).expect("debe extraer URL yourupload");
            assert_eq!(url, "https://vidcache.net:8161/a20261114hh20TMKBVwr/video.mp4");
        }
    }
}

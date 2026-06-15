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

use anyhow::{Context, Result};
use regex::Regex;
use std::sync::OnceLock;

use crate::logger;
use crate::scraper::client::build_client;

// ————————————————————————————————————————————————
// Tipos de resultado de resolución
// ————————————————————————————————————————————————

/// Resultado de intentar resolver una URL de embed.
#[derive(Debug, Clone)]
pub enum ResolvedUrl {
    /// URL directa de stream (m3u8, mp4) reproducible con mpv.
    /// Puede incluir headers adicionales necesarios.
    DirectStream {
        url: String,
        /// Header Referer necesario para la petición (si aplica)
        referer: Option<String>,
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
    /// Desconocido
    Unknown,
}

impl StreamType {
    pub fn display(&self) -> &str {
        match self {
            StreamType::Hls => "HLS (m3u8)",
            StreamType::Mp4 => "MP4 directo",
            StreamType::Unknown => "Desconocido",
        }
    }
}

// ————————————————————————————————————————————————
// Función principal de resolución
// ————————————————————————————————————————————————

/// Intenta resolver la URL de embed/player a una URL directa reproducible.
///
/// # Parámetros
/// - `server`: Nombre del servidor (ej: "HLS", "PDrain", "MP4Upload")
/// - `embed_url`: URL del embed tal como viene en el HTML del sitio
///
/// # Retorna
/// `ResolvedUrl::DirectStream` si se pudo extraer una URL directa,
/// `ResolvedUrl::FallbackBrowser` si hay que abrir en el navegador.
pub async fn resolve_url(server: &str, embed_url: &str) -> ResolvedUrl {
    logger::log_info("resolver", &format!(
        "Resolviendo servidor={} url={}",
        server,
        // Loguear solo el inicio de la URL por seguridad
        &embed_url[..embed_url.len().min(60)]
    ));

    // SEGURIDAD: Validar que la URL sea HTTP/HTTPS antes de procesarla
    if !embed_url.starts_with("https://") && !embed_url.starts_with("http://") {
        logger::log_security("resolver", &format!(
            "URL rechazada: no es http/https — servidor={}", server
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "URL con esquema no permitido (solo http/https)".to_string(),
        };
    }

    match server {
        "HLS" => resolve_zilla_hls(embed_url).await,
        "PDrain" => resolve_pixeldrain(embed_url),
        "MP4Upload" => resolve_mp4upload(embed_url).await,
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
        // El ID es 32 caracteres hexadecimales al final del path
        Regex::new(r"/play/([0-9a-f]{32})").expect("regex zilla_id inválida")
    })
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
async fn resolve_zilla_hls(embed_url: &str) -> ResolvedUrl {
    // Extraer el ID de 32 chars del path
    let id = match get_re_zilla_id().captures(embed_url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
    {
        Some(id) => id,
        None => {
            // Intentar el formato alternativo (si la URL ya es un m3u8)
            if embed_url.contains("/m3u8/") {
                logger::log_info("resolver::zilla", "URL ya es m3u8 directo");
                return ResolvedUrl::DirectStream {
                    url: embed_url.to_string(),
                    referer: Some(embed_url.replace("/m3u8/", "/play/")),
                    content_type: StreamType::Hls,
                };
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
    // para prevenir path traversal u otras inyecciones de URL
    if id.len() != 32 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        logger::log_security("resolver::zilla", &format!(
            "ID rechazado: formato inválido (len={}, id={})", id.len(), &id[..id.len().min(10)]
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "ID del player con formato inválido (posible ataque de path traversal)".to_string(),
        };
    }

    // Construir la URL m3u8 directa
    let m3u8_url = format!("https://player.zilla-networks.com/m3u8/{}", id);
    let referer = format!("https://player.zilla-networks.com/play/{}", id);

    logger::log_info("resolver::zilla", &format!(
        "URL m3u8 resuelta para ID {} — tipo: HLS", &id[..8]
    ));

    ResolvedUrl::DirectStream {
        url: m3u8_url,
        referer: Some(referer),
        content_type: StreamType::Hls,
    }
}

// ————————————————————————————————————————————————
// PixelDrain
// ————————————————————————————————————————————————

/// Regex para extraer el file ID de PixelDrain.
/// Ejemplo: https://pixeldrain.com/u/MJzY7Ab1?embed
static RE_PIXELDRAIN_ID: OnceLock<Regex> = OnceLock::new();

fn get_re_pixeldrain_id() -> &'static Regex {
    RE_PIXELDRAIN_ID.get_or_init(|| {
        Regex::new(r"pixeldrain\.com/u/([A-Za-z0-9]+)").expect("regex pixeldrain_id inválida")
    })
}

/// Resuelve la URL de embed de PixelDrain a la URL de descarga directa.
///
/// PixelDrain tiene una API pública:
///   Embed:    https://pixeldrain.com/u/{file-id}?embed
///   Directo:  https://pixeldrain.com/api/file/{file-id}
fn resolve_pixeldrain(embed_url: &str) -> ResolvedUrl {
    resolve_pixeldrain_internal(embed_url)
}

/// Versión pública para uso desde player.rs en contextos síncronos.
pub fn resolve_pixeldrain_sync(embed_url: &str) -> ResolvedUrl {
    resolve_pixeldrain_internal(embed_url)
}

fn resolve_pixeldrain_internal(embed_url: &str) -> ResolvedUrl {
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
    if file_id.len() > 20 || !file_id.chars().all(|c| c.is_alphanumeric()) {
        logger::log_security("resolver::pixeldrain", &format!(
            "File ID rechazado: formato inválido ({})", &file_id[..file_id.len().min(10)]
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "File ID de PixelDrain con formato inválido".to_string(),
        };
    }

    let direct_url = format!("https://pixeldrain.com/api/file/{}", file_id);
    logger::log_info("resolver::pixeldrain", &format!(
        "URL directa resuelta para ID {}", file_id
    ));

    ResolvedUrl::DirectStream {
        url: direct_url,
        referer: None,
        content_type: StreamType::Mp4,
    }
}

// ————————————————————————————————————————————————
// MP4Upload
// ————————————————————————————————————————————————

/// Regex para extraer la URL de video del HTML de MP4Upload.
/// Busca el src del elemento <source> dentro del player.
static RE_MP4UPLOAD_SRC: OnceLock<Regex> = OnceLock::new();

fn get_re_mp4upload_src() -> &'static Regex {
    RE_MP4UPLOAD_SRC.get_or_init(|| {
        // Busca src="https://....mp4" dentro del HTML del embed
        Regex::new(r#"src:"(https://[^"]+\.mp4[^"]*)""#).expect("regex mp4upload_src inválida")
    })
}

/// Intenta extraer la URL del MP4 directamente del HTML del embed de MP4Upload.
///
/// MP4Upload renderiza el HTML del player con un <source src="..."> que contiene
/// la URL directa del MP4. Se hace un fetch del embed y se extrae con regex.
///
/// NOTA: Si MP4Upload cambia su HTML, esta extracción puede fallar.
/// En ese caso, el fallback es abrir en el navegador.
async fn resolve_mp4upload(embed_url: &str) -> ResolvedUrl {
    // SEGURIDAD: Verificar que la URL es realmente de mp4upload
    if !embed_url.contains("mp4upload.com") {
        logger::log_security("resolver::mp4upload", &format!(
            "URL rechazada: no es de mp4upload.com ({})", &embed_url[..embed_url.len().min(40)]
        ));
        return ResolvedUrl::FallbackBrowser {
            url: embed_url.to_string(),
            reason: "URL no es de mp4upload.com".to_string(),
        };
    }

    logger::log_net("GET", embed_url, "iniciando fetch MP4Upload...");

    let client = match build_client() {
        Ok(c) => c,
        Err(e) => {
            logger::log_error("resolver::mp4upload", &format!("Error cliente HTTP: {}", e));
            return ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason: format!("Error al crear cliente HTTP: {}", e),
            };
        }
    };

    let html = match client.get(embed_url)
        .header("Referer", "https://animeav1.com/")
        .send()
        .await
        .and_then(|r| Ok(r))
    {
        Ok(resp) => {
            let status = resp.status().to_string();
            logger::log_net("GET", embed_url, &status);
            match resp.text().await {
                Ok(text) => text,
                Err(e) => {
                    logger::log_error("resolver::mp4upload", &format!("Error leyendo respuesta: {}", e));
                    return ResolvedUrl::FallbackBrowser {
                        url: embed_url.to_string(),
                        reason: format!("Error al leer HTML de MP4Upload: {}", e),
                    };
                }
            }
        }
        Err(e) => {
            logger::log_error("resolver::mp4upload", &format!("Error fetch: {}", e));
            return ResolvedUrl::FallbackBrowser {
                url: embed_url.to_string(),
                reason: format!("Error de red al acceder a MP4Upload: {}", e),
            };
        }
    };

    // Extraer URL del MP4 del HTML
    match get_re_mp4upload_src().captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
    {
        Some(mp4_url) => {
            // SEGURIDAD: Verificar que la URL extraída es HTTP/HTTPS
            if !mp4_url.starts_with("https://") && !mp4_url.starts_with("http://") {
                logger::log_security("resolver::mp4upload", "URL extraída no es http/https");
                return ResolvedUrl::FallbackBrowser {
                    url: embed_url.to_string(),
                    reason: "URL de MP4 extraída con esquema no permitido".to_string(),
                };
            }

            logger::log_info("resolver::mp4upload", "URL MP4 directa extraída correctamente");
            ResolvedUrl::DirectStream {
                url: mp4_url,
                referer: Some("https://www.mp4upload.com/".to_string()),
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

// ============================================================
// src/player.rs — Reproducción de video
// ============================================================
//
// Gestiona la reproducción de fuentes de video usando:
//   1. Resolver de URL: convierte embed → URL directa (m3u8/mp4)
//   2. mpv (si está instalado) — reproduce sin anuncios
//   3. Navegador del sistema — fallback automático
//
// Flujo de reproducción:
//   embed_url
//     │
//     ▼
//   resolver::resolve_url()
//     │
//     ├─ DirectStream (m3u8/mp4) → intentar mpv con Referer
//     │                          → si falla: abrir en navegador
//     │
//     └─ FallbackBrowser → abrir directamente en navegador
//
// Por qué el problema original: la URL del servidor "HLS" es
// https://player.zilla-networks.com/PLAY/{id} (página HTML del player),
// NO la URL del m3u8. mpv no puede reproducir una página HTML.
// El resolver extrae la URL m3u8 real antes de lanzar mpv.
// ============================================================

use anyhow::{Context, Result};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use crate::config::USER_AGENT;
use crate::logger;
use crate::resolver::{resolve_url, ResolvedUrl};
use crate::structs::{truncate_chars, VideoSource};

// ————————————————————————————————————————————————
// Verificación de disponibilidad de mpv
// ————————————————————————————————————————————————

static MPV_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Verifica si `mpv` está instalado y ejecutable en el PATH del sistema.
/// Cachea el resultado durante la ejecución para evitar lanzar subprocesos repetidos.
pub fn is_mpv_available() -> bool {
    *MPV_AVAILABLE.get_or_init(|| {
        let available = Command::new("mpv")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        logger::log_info(
            "player",
            &format!(
                "mpv disponible: {}",
                if available {
                    "SÍ"
                } else {
                    "NO (instalar con: sudo apt install mpv)"
                }
            ),
        );

        available
    })
}

// ————————————————————————————————————————————————
// Resultado de reproducción
// ————————————————————————————————————————————————

/// Resultado de intentar reproducir una fuente de video.
#[derive(Debug)]
pub enum PlayResult {
    /// mpv lanzado correctamente en background
    LaunchedMpv { url: String },
    /// Navegador del sistema abierto
    LaunchedBrowser { url: String },
    /// No hay fuentes del tipo de audio solicitado
    NoSources,
    /// Error al intentar reproducir
    Error(String),
}

impl PlayResult {
    /// Mensaje descriptivo del resultado para mostrar en la TUI.
    pub fn display_message(&self) -> String {
        match self {
            PlayResult::LaunchedMpv { .. } => "▶ Reproduciendo con mpv (sin anuncios)".to_string(),
            PlayResult::LaunchedBrowser { .. } => "🌐 Abriendo en el navegador...".to_string(),
            PlayResult::NoSources => "⚠ No hay fuentes disponibles para este audio".to_string(),
            PlayResult::Error(e) => format!("✗ Error: {}", e),
        }
    }
}

// ————————————————————————————————————————————————
// Reproducción asíncrona (con resolución de URL)
// ————————————————————————————————————————————————

/// Reproduce una fuente de video resolviendo primero su URL real.
///
/// # Flujo
/// 1. Llama a `resolver::resolve_url()` para convertir embed → stream directo
/// 2. Si es DirectStream → intenta mpv con User-Agent, Referer y headers extra
/// 3. Si mpv falla inmediatamente o no está disponible → abre `source.url` en navegador
/// 4. Si es FallbackBrowser → abre directamente en navegador
pub async fn play_source_async(source: &VideoSource) -> PlayResult {
    logger::log_info("player", &format!(
        "Iniciando reproducción: servidor={}, audio={}",
        source.server,
        source.audio.display()
    ));

    let resolved = resolve_url(&source.server, &source.url).await;

    match resolved {
        ResolvedUrl::DirectStream {
            url,
            referer,
            headers,
            content_type,
        } => {
            logger::log_info("player", &format!(
                "URL resuelta — tipo: {}, referer: {}, headers: {}",
                content_type.display(),
                referer.as_deref().unwrap_or("ninguno"),
                headers.len()
            ));

            if is_mpv_available() {
                match launch_mpv_async(&url, referer.as_deref(), &headers).await {
                    Ok(()) => return PlayResult::LaunchedMpv { url },
                    Err(e) => {
                        logger::log_warn("player", &format!(
                            "mpv falló ({}), usando navegador...", e
                        ));
                    }
                }
            } else {
                logger::log_info("player", "mpv no disponible → usando navegador");
            }

            // En fallback web abrir la URL original del embed/player resulta más fiable
            let browser_url = source.url.clone();
            match open_in_browser(&browser_url) {
                Ok(()) => PlayResult::LaunchedBrowser { url: browser_url },
                Err(e) => PlayResult::Error(e.to_string()),
            }
        }

        ResolvedUrl::FallbackBrowser { url, reason } => {
            logger::log_info("player", &format!(
                "Usando navegador directamente: {}", reason
            ));
            match open_in_browser(&url) {
                Ok(()) => PlayResult::LaunchedBrowser { url },
                Err(e) => PlayResult::Error(e.to_string()),
            }
        }
    }
}

// ————————————————————————————————————————————————
// Lanzar mpv
// ————————————————————————————————————————————————

fn has_control_chars(s: &str) -> bool {
    s.chars().any(|c| c == '\r' || c == '\n' || c == '\0')
}

/// Construye la lista de argumentos seguros para lanzar `mpv`.
pub(crate) fn build_mpv_args(
    url: &str,
    referer: Option<&str>,
    headers: &[(String, String)],
) -> Result<Vec<String>> {
    // SEGURIDAD: Validar que la URL es http/https sin caracteres de control
    if (!url.starts_with("https://") && !url.starts_with("http://")) || has_control_chars(url) {
        let msg = format!(
            "URL rechazada por seguridad (no http/https): {}",
            truncate_chars(url, 40)
        );
        logger::log_security("player::mpv", &msg);
        return Err(anyhow::anyhow!(msg));
    }

    let mut args = vec![
        "--no-terminal".to_string(),
        "--really-quiet".to_string(),
        "--force-window=immediate".to_string(),
        format!("--user-agent={}", USER_AGENT),
    ];

    if let Some(ref_url) = referer {
        if (ref_url.starts_with("https://") || ref_url.starts_with("http://"))
            && !has_control_chars(ref_url)
        {
            args.push(format!("--referrer={}", ref_url));
        }
    }

    for (name, value) in headers {
        if !name.is_empty() && !has_control_chars(name) && !has_control_chars(value) {
            args.push(format!("--http-header-fields-append={}: {}", name, value));
        }
    }

    args.push(url.to_string());
    Ok(args)
}

fn spawn_mpv_child(
    url: &str,
    referer: Option<&str>,
    headers: &[(String, String)],
) -> Result<Child> {
    let args = build_mpv_args(url, referer, headers)?;

    let mut cmd = Command::new("mpv");
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    logger::log_cmd(
        "mpv",
        &[
            "--no-terminal",
            "--really-quiet",
            "--force-window=immediate",
            if referer.is_some() { "--referrer=[referer]" } else { "--user-agent=[ua]" },
            if headers.is_empty() { "[url]" } else { "--http-header-fields-append=[headers] [url]" },
        ],
        "lanzando...",
    );

    cmd.spawn().context("Error al lanzar mpv")
}

/// Lanza `mpv` en background, espera brevemente para detectar fallos inmediatos
/// de red/arranque (ej. HTTP 403/404 rápido) y cosecha el proceso al finalizar.
async fn launch_mpv_async(
    url: &str,
    referer: Option<&str>,
    headers: &[(String, String)],
) -> Result<()> {
    let mut child = spawn_mpv_child(url, referer, headers)?;

    // Pausa corta no bloqueante para detectar cierre prematuro por error del stream
    tokio::time::sleep(Duration::from_millis(450)).await;

    if let Some(status) = child.try_wait().context("Error comprobando estado de mpv")? {
        return Err(anyhow::anyhow!("mpv salió prematuramente con estado {}", status));
    }

    logger::log_info("player::mpv", "mpv iniciado y reproduciendo en background");

    // Evitar procesos zombie al cerrar la ventana de mpv durante la sesión TUI
    tokio::task::spawn_blocking(move || {
        match child.wait() {
            Ok(status) => logger::log_info("player::mpv", &format!("mpv finalizó: {}", status)),
            Err(e) => logger::log_warn("player::mpv", &format!("Error esperando a mpv: {}", e)),
        }
    });

    Ok(())
}

// ————————————————————————————————————————————————
// Abrir en navegador
// ————————————————————————————————————————————————

/// Abre una URL en el navegador predeterminado del sistema.
///
/// SEGURIDAD: Usa la crate `open` que internamente usa xdg-open en Linux,
/// lo que es más seguro que ejecutar directamente un navegador específico.
/// La crate `open` pasa la URL como argumento del proceso (no como shell string),
/// previniendo inyección de comandos.
fn open_in_browser(url: &str) -> Result<()> {
    // SEGURIDAD: Validar esquema antes de abrir
    if (!url.starts_with("https://") && !url.starts_with("http://")) || has_control_chars(url) {
        let msg = format!(
            "URL rechazada (esquema no http/https): {}",
            truncate_chars(url, 40)
        );
        logger::log_security("player::browser", &msg);
        return Err(anyhow::anyhow!(msg));
    }

    logger::log_cmd("xdg-open", &["[url]"], "abriendo navegador...");

    open::that(url)
        .map(|_| {
            logger::log_info("player::browser", "Navegador abierto exitosamente");
        })
        .context("Error al abrir el navegador del sistema")?;

    Ok(())
}

// ————————————————————————————————————————————————
// Información del sistema
// ————————————————————————————————————————————————

/// Información sobre reproductores disponibles en el sistema.
#[derive(Debug)]
pub struct PlayerInfo {
    /// Si `mpv` está instalado y disponible
    pub mpv_available: bool,
}

/// Devuelve información sobre las capacidades de reproducción del sistema.
pub fn player_info() -> PlayerInfo {
    PlayerInfo {
        mpv_available: is_mpv_available(),
    }
}

impl PlayerInfo {
    /// Descripción del estado del reproductor para mostrar en la TUI.
    pub fn status_line(&self) -> &'static str {
        if self.mpv_available {
            "mpv disponible — reproducción directa sin anuncios"
        } else {
            "mpv no encontrado — se usará el navegador del sistema"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_mpv_args_includes_ua_referer_headers_and_force_window() {
        let headers = vec![
            ("Origin".to_string(), "https://player.zilla-networks.com".to_string()),
            ("Sec-Fetch-Site".to_string(), "same-origin".to_string()),
        ];
        let args = build_mpv_args(
            "https://player.zilla-networks.com/m3u8/7601658844858539e438dceee32e7924",
            Some("https://player.zilla-networks.com/play/7601658844858539e438dceee32e7924"),
            &headers,
        )
        .unwrap();

        assert!(args.contains(&"--no-terminal".to_string()));
        assert!(args.contains(&"--really-quiet".to_string()));
        assert!(args.contains(&"--force-window=immediate".to_string()));
        assert!(args.iter().any(|a| a.starts_with("--user-agent=Mozilla/")));
        assert!(args.contains(&"--referrer=https://player.zilla-networks.com/play/7601658844858539e438dceee32e7924".to_string()));
        assert!(args.contains(&"--http-header-fields-append=Origin: https://player.zilla-networks.com".to_string()));
        assert!(args.contains(&"--http-header-fields-append=Sec-Fetch-Site: same-origin".to_string()));
    }

    #[test]
    fn build_mpv_args_rejects_bad_scheme_and_crlf_headers() {
        assert!(build_mpv_args("file:///etc/passwd", None, &[]).is_err());
        let bad_headers = vec![("Origin".to_string(), "https://ok\r\nEvil: 1".to_string())];
        let args = build_mpv_args("https://example.com/v.mp4", None, &bad_headers).unwrap();
        assert!(!args.iter().any(|a| a.contains("Evil:")));
    }
}

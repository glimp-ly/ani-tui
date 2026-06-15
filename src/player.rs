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
use std::process::{Command, Stdio};

use crate::logger;
use crate::resolver::{resolve_url, ResolvedUrl, StreamType};
use crate::structs::VideoSource;

// ————————————————————————————————————————————————
// Verificación de disponibilidad de mpv
// ————————————————————————————————————————————————

/// Verifica si `mpv` está instalado y disponible en el PATH del sistema.
pub fn is_mpv_available() -> bool {
    let available = Command::new("which")
        .arg("mpv")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    logger::log_info("player", &format!(
        "mpv disponible: {}", if available { "SÍ" } else { "NO (instalar con: sudo apt install mpv)" }
    ));

    available
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
/// Este es el punto de entrada principal para reproducción.
/// La resolución de URL puede ser asíncrona (fetch HTTP para MP4Upload).
///
/// # Flujo
/// 1. Llama a `resolver::resolve_url()` para convertir embed → stream directo
/// 2. Si es DirectStream → intenta mpv (con headers Referer si los hay)
/// 3. Si mpv falla o no está disponible → abre en navegador
/// 4. Si es FallbackBrowser → abre en navegador directamente
pub async fn play_source_async(source: &VideoSource) -> PlayResult {
    logger::log_info("player", &format!(
        "Iniciando reproducción: servidor={}, audio={}",
        source.server,
        source.audio.display()
    ));

    // Resolver la URL embed → URL directa reproducible
    let resolved = resolve_url(&source.server, &source.url).await;

    match resolved {
        ResolvedUrl::DirectStream { url, referer, content_type } => {
            logger::log_info("player", &format!(
                "URL resuelta — tipo: {}, referer: {}",
                content_type.display(),
                referer.as_deref().unwrap_or("ninguno")
            ));

            // Intentar con mpv primero
            if is_mpv_available() {
                match launch_mpv(&url, referer.as_deref()) {
                    Ok(()) => {
                        return PlayResult::LaunchedMpv { url };
                    }
                    Err(e) => {
                        logger::log_warn("player", &format!(
                            "mpv falló ({}), usando navegador...", e
                        ));
                    }
                }
            } else {
                logger::log_info("player", "mpv no disponible → usando navegador");
            }

            // Para HLS, abrir el player original (mejor experiencia web)
            // Usamos unwrap_or ya que is_some() fue verificado en la condición
            let browser_url = if content_type == StreamType::Hls && referer.is_some() {
                referer.unwrap_or(url.clone())
            } else {
                url.clone()
            };

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

/// Versión síncrona de reproducción (para compatibilidad con el event loop de la TUI).
/// Usa tokio::task::block_in_place para ejecutar código async en contexto síncrono.
pub fn play_source(source: &VideoSource) -> PlayResult {
    // Usar el runtime de tokio existente para ejecutar la resolución async
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            // Estamos dentro de un runtime de tokio — usar block_in_place
            tokio::task::block_in_place(|| {
                handle.block_on(play_source_async(source))
            })
        }
        Err(_) => {
            // No hay runtime — fallback síncrono sin resolver
            logger::log_warn("player", "No hay runtime tokio disponible — resolución limitada");
            play_source_sync_fallback(source)
        }
    }
}

/// Fallback síncrono cuando no hay runtime de tokio disponible.
/// Solo puede manejar PixelDrain (resolución sin HTTP) y navegador.
fn play_source_sync_fallback(source: &VideoSource) -> PlayResult {
    // Para PixelDrain podemos resolver sin async
    if source.server == "PDrain" {
        if let crate::resolver::ResolvedUrl::DirectStream { url, .. } =
            crate::resolver::resolve_pixeldrain_sync(&source.url)
        {
            if is_mpv_available() {
                if launch_mpv(&url, None).is_ok() {
                    return PlayResult::LaunchedMpv { url };
                }
            }
        }
    }

    // Para todos los demás: navegador
    match open_in_browser(&source.url) {
        Ok(()) => PlayResult::LaunchedBrowser { url: source.url.clone() },
        Err(e) => PlayResult::Error(e.to_string()),
    }
}

// ————————————————————————————————————————————————
// Lanzar mpv
// ————————————————————————————————————————————————

/// Lanza mpv para reproducir la URL dada.
///
/// Opciones utilizadas:
/// - `--no-terminal`: No mostrar barra de progreso en la terminal (preserva la TUI)
/// - `--really-quiet`: Silenciar output de mpv que interferiría con la TUI
/// - `--referrer=<url>`: Header Referer para URLs que lo requieren (como HLS de Zilla)
/// - spawn() en background: no bloquea la TUI mientras mpv está abierto
///
/// SEGURIDAD: Los argumentos se pasan como array (no como string de shell),
/// lo que previene command injection aunque la URL contenga caracteres especiales.
fn launch_mpv(url: &str, referer: Option<&str>) -> Result<()> {
    // SEGURIDAD: Validar que la URL es http/https antes de pasarla a mpv
    if !url.starts_with("https://") && !url.starts_with("http://") {
        let msg = format!("URL rechazada por seguridad (no http/https): {}", &url[..url.len().min(40)]);
        logger::log_security("player::mpv", &msg);
        return Err(anyhow::anyhow!(msg));
    }

    let mut cmd = Command::new("mpv");

    // Argumentos base de mpv
    cmd.arg("--no-terminal") // No interferir con la TUI
       .arg("--really-quiet"); // Silenciar output

    // Agregar Referer si es necesario (para URLs de Zilla Networks)
    if let Some(ref_url) = referer {
        // SEGURIDAD: Validar el Referer también
        if ref_url.starts_with("https://") || ref_url.starts_with("http://") {
            cmd.arg(format!("--referrer={}", ref_url));
        }
    }

    // URL del stream (último argumento)
    cmd.arg(url);

    // Desconectar de stdin/stdout/stderr de la TUI
    cmd.stdin(Stdio::null())
       .stdout(Stdio::null())
       .stderr(Stdio::null());

    // Log del comando (sin la URL completa por privacidad)
    let has_referer = referer.is_some();
    if has_referer {
        logger::log_cmd("mpv", &["--no-terminal", "--really-quiet", "--referrer=[referer]", "[url]"], "lanzando...");
    } else {
        logger::log_cmd("mpv", &["--no-terminal", "--really-quiet", "[url]"], "lanzando...");
    }

    cmd.spawn()
       .map(|_| {
           logger::log_info("player::mpv", "mpv lanzado exitosamente en background");
       })
       .context("Error al lanzar mpv")?;

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
    if !url.starts_with("https://") && !url.starts_with("http://") {
        let msg = format!("URL rechazada (esquema no http/https): {}", &url[..url.len().min(40)]);
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
    pub fn status_line(&self) -> String {
        if self.mpv_available {
            "🎬 mpv disponible — reproducción directa sin anuncios".to_string()
        } else {
            "⚠️  mpv no encontrado — se usará el navegador del sistema".to_string()
        }
    }
}

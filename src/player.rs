// ============================================================
// src/player.rs — Reproducción de video
// ============================================================
//
// Gestiona la reproducción de fuentes de video usando:
//   1. mpv (si está instalado) — reproduce directamente, sin anuncios
//   2. Navegador del sistema   — fallback cuando mpv no está disponible
//      o cuando el servidor no es compatible con mpv
//
// Servidores compatibles con mpv directamente:
//   - HLS (player.zilla-networks.com) — stream m3u8
//   - PDrain (PixelDrain)             — enlace directo
//
// Servidores que requieren el navegador:
//   - Mega, MP4Upload, UPNShare, TeraBox — iframes embed
// ============================================================

use anyhow::{Context, Result};
use std::process::{Command, Stdio};

use crate::structs::{AudioType, EpisodeSources, VideoSource};

// ————————————————————————————————————————————————
// Verificación de disponibilidad de mpv
// ————————————————————————————————————————————————

/// Verifica si `mpv` está instalado y disponible en el PATH del sistema.
///
/// Se llama una vez al inicio de la TUI para saber si se puede
/// ofrecer reproducción directa sin anuncios.
pub fn is_mpv_available() -> bool {
    Command::new("which")
        .arg("mpv")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ————————————————————————————————————————————————
// Reproducción con prioridad mpv → navegador
// ————————————————————————————————————————————————

/// Resultado de intentar reproducir una fuente de video.
#[derive(Debug)]
pub enum PlayResult {
    /// Se lanzó mpv correctamente
    LaunchedMpv,
    /// Se abrió en el navegador del sistema
    LaunchedBrowser,
    /// No hay fuentes disponibles del tipo solicitado
    NoSources,
    /// Error al intentar reproducir
    Error(String),
}

/// Intenta reproducir el episodio usando la mejor fuente disponible.
///
/// Estrategia de selección de fuente y reproductor:
/// 1. Si hay fuente HLS → intentar con mpv primero
/// 2. Si mpv falla o no está disponible → abrir HLS en navegador
/// 3. Si no hay HLS pero hay PDrain → intentar con mpv
/// 4. Para cualquier otra fuente → abrir en navegador
///
/// # Parámetros
/// - `sources`: Las fuentes disponibles del episodio
/// - `audio`: Tipo de audio preferido (SUB o DUB)
/// - `preferred_server`: Servidor específico preferido (None = automático)
pub fn play_episode(
    sources: &EpisodeSources,
    audio: &AudioType,
    preferred_server: Option<&str>,
) -> PlayResult {
    let available = sources.get(audio);

    if available.is_empty() {
        return PlayResult::NoSources;
    }

    // — Seleccionar fuente —
    let source = if let Some(server_name) = preferred_server {
        // El usuario seleccionó un servidor específico
        available.iter().find(|s| s.server == server_name)
    } else {
        // Selección automática: priorizar HLS, luego PDrain, luego cualquiera
        available.iter().find(|s| s.server == "HLS")
            .or_else(|| available.iter().find(|s| s.server == "PDrain"))
            .or_else(|| available.first())
    };

    let source = match source {
        Some(s) => s,
        None => return PlayResult::NoSources,
    };

    // — Intentar reproducir —
    play_source(source)
}

/// Reproduce una fuente de video específica.
///
/// Si la fuente es compatible con mpv, intenta con mpv primero.
/// Si mpv falla o no está disponible, abre en el navegador.
pub fn play_source(source: &VideoSource) -> PlayResult {
    if source.is_mpv_compatible() && is_mpv_available() {
        // Intentar con mpv
        match launch_mpv(&source.url) {
            Ok(()) => return PlayResult::LaunchedMpv,
            Err(e) => {
                // mpv disponible pero falló — intentar con navegador
                eprintln!("[player] mpv falló ({}), usando navegador...", e);
            }
        }
    }

    // Fallback: abrir en navegador del sistema
    match open_in_browser(&source.url) {
        Ok(()) => PlayResult::LaunchedBrowser,
        Err(e) => PlayResult::Error(e.to_string()),
    }
}

/// Abre una URL directamente por servidor específico.
/// Útil cuando el usuario elige manualmente un servidor que no es mpv-compatible.
pub fn open_url(url: &str, try_mpv: bool) -> PlayResult {
    if try_mpv && is_mpv_available() {
        match launch_mpv(url) {
            Ok(()) => return PlayResult::LaunchedMpv,
            Err(_) => {}
        }
    }

    match open_in_browser(url) {
        Ok(()) => PlayResult::LaunchedBrowser,
        Err(e) => PlayResult::Error(e.to_string()),
    }
}

// ————————————————————————————————————————————————
// Lanzar mpv
// ————————————————————————————————————————————————

/// Lanza mpv para reproducir la URL dada.
///
/// Opciones de mpv utilizadas:
/// - `--no-terminal`: No mostrar la barra de progreso de mpv en la terminal
///   (la TUI necesita control exclusivo del terminal)
/// - `--really-quiet`: Silenciar mensajes de mpv que interferirían con la TUI
/// - `--title=<url>`: Establecer el título de la ventana de mpv
///
/// mpv se lanza en segundo plano para no bloquear la TUI.
fn launch_mpv(url: &str) -> Result<()> {
    Command::new("mpv")
        .args([
            "--no-terminal",  // No interferir con la TUI
            "--really-quiet", // Silenciar output
            url,
        ])
        // Desconectar stdin/stdout/stderr de la TUI
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // spawn() inicia en background (no bloqueante)
        .spawn()
        .context("Error al lanzar mpv")?;

    Ok(())
}

// ————————————————————————————————————————————————
// Abrir en navegador
// ————————————————————————————————————————————————

/// Abre una URL en el navegador predeterminado del sistema.
///
/// Usa la crate `open` que internamente usa:
/// - Linux: xdg-open
/// - macOS: open
/// - Windows: start
fn open_in_browser(url: &str) -> Result<()> {
    open::that(url)
        .context("Error al abrir el navegador del sistema")?;
    Ok(())
}

// ————————————————————————————————————————————————
// Información de diagnóstico del sistema
// ————————————————————————————————————————————————

/// Devuelve información sobre las capacidades de reproducción del sistema.
/// Útil para mostrar en la pantalla de ayuda de la TUI.
pub fn player_info() -> PlayerInfo {
    PlayerInfo {
        mpv_available: is_mpv_available(),
    }
}

/// Información sobre reproductores disponibles en el sistema.
#[derive(Debug)]
pub struct PlayerInfo {
    /// Si `mpv` está disponible para reproducción directa sin anuncios
    pub mpv_available: bool,
}

impl PlayerInfo {
    /// Descripción legible del estado del reproductor para la TUI.
    pub fn status_line(&self) -> String {
        if self.mpv_available {
            "🎬 mpv disponible — reproducción directa sin anuncios".to_string()
        } else {
            "⚠️  mpv no encontrado — se usará el navegador del sistema".to_string()
        }
    }
}

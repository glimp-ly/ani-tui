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
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

use crate::config::USER_AGENT;
use crate::logger;
use crate::resolver::{resolve_url, ResolvedUrl};
use crate::storage::format_duration;
use crate::structs::{truncate_chars, VideoSource};

static IPC_SOCKET_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Progreso de reproducción reportado por `mpv` mediante IPC.
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub anime_slug: String,
    pub episode_number: u32,
    pub position_secs: u64,
    pub duration_secs: Option<u64>,
    pub ended_eof: bool,
}

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
    LaunchedMpv {
        url: String,
        started_at_secs: Option<u64>,
    },
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
            PlayResult::LaunchedMpv { started_at_secs, .. } => match started_at_secs {
                Some(secs) if *secs >= 5 => {
                    format!("▶ Reproduciendo con mpv desde {}", format_duration(*secs))
                }
                _ => "▶ Reproduciendo con mpv (sin anuncios)".to_string(),
            },
            PlayResult::LaunchedBrowser { .. } => "🌐 Abriendo en el navegador...".to_string(),
            PlayResult::NoSources => "⚠ No hay fuentes disponibles para este audio".to_string(),
            PlayResult::Error(e) => format!("✗ Error: {}", e),
        }
    }
}

/// Metadatos opcionales para reanudar y registrar el minuto visto en `mpv`.
pub struct PlayOptions {
    pub anime_slug: String,
    pub episode_number: u32,
    pub start_secs: Option<u64>,
    pub progress_tx: Option<UnboundedSender<ProgressUpdate>>,
}

// ————————————————————————————————————————————————
// Reproducción asíncrona (con resolución de URL)
// ————————————————————————————————————————————————

/// Reproduce una fuente de video resolviendo primero su URL real.
///
/// # Flujo
/// 1. Llama a `resolver::resolve_url()` para convertir embed → stream directo
/// 2. Si es DirectStream → intenta mpv con User-Agent, Referer, headers y `--start` si aplica
/// 3. Si mpv falla inmediatamente o no está disponible → abre `source.url` en navegador
/// 4. Si es FallbackBrowser → abre directamente en navegador
pub async fn play_source_async(source: &VideoSource, options: PlayOptions) -> PlayResult {
    logger::log_info("player", &format!(
        "Iniciando reproducción: servidor={}, audio={}, start={:?}",
        source.server,
        source.audio.display(),
        options.start_secs
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
                match launch_mpv_async(&url, referer.as_deref(), &headers, &options).await {
                    Ok(()) => {
                        return PlayResult::LaunchedMpv {
                            url,
                            started_at_secs: options.start_secs,
                        };
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
    start_secs: Option<u64>,
    ipc_socket: Option<&Path>,
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

    if let Some(secs) = start_secs.filter(|&s| s >= 5) {
        args.push(format!("--start={}", secs));
    }

    if let Some(sock_str) = ipc_socket.and_then(|p| p.to_str()) {
        if !sock_str.is_empty() && !has_control_chars(sock_str) {
            args.push(format!("--input-ipc-server={}", sock_str));
        }
    }

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

fn next_ipc_socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let seq = IPC_SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
    base.join(format!("ani-tui-mpv-{}-{}.sock", std::process::id(), seq))
}

fn spawn_mpv_child(
    url: &str,
    referer: Option<&str>,
    headers: &[(String, String)],
    start_secs: Option<u64>,
    ipc_socket: Option<&Path>,
) -> Result<Child> {
    let args = build_mpv_args(url, referer, headers, start_secs, ipc_socket)?;

    let mut cmd = Command::new("mpv");
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    logger::log_cmd(
        "mpv",
        &[
            "--no-terminal",
            "--force-window=immediate",
            if start_secs.is_some_and(|s| s >= 5) { "--start=[secs]" } else { "" },
            if referer.is_some() { "--referrer=[referer]" } else { "--user-agent=[ua]" },
            if headers.is_empty() { "[url]" } else { "--http-header-fields-append=[headers] [url]" },
        ],
        "lanzando...",
    );

    cmd.spawn().context("Error al lanzar mpv")
}

/// Lanza `mpv` en background, espera brevemente para detectar fallos inmediatos
/// de red/arranque (ej. HTTP 403/404 rápido) y monitoriza posición vía IPC.
async fn launch_mpv_async(
    url: &str,
    referer: Option<&str>,
    headers: &[(String, String)],
    options: &PlayOptions,
) -> Result<()> {
    let ipc_path = if cfg!(unix) && options.progress_tx.is_some() {
        let p = next_ipc_socket_path();
        let _ = std::fs::remove_file(&p);
        Some(p)
    } else {
        None
    };

    let mut child = spawn_mpv_child(
        url,
        referer,
        headers,
        options.start_secs,
        ipc_path.as_deref(),
    )?;

    // Pausa corta no bloqueante para detectar cierre prematuro por error del stream
    tokio::time::sleep(Duration::from_millis(450)).await;

    if let Some(status) = child.try_wait().context("Error comprobando estado de mpv")? {
        if let Some(ref p) = ipc_path {
            let _ = std::fs::remove_file(p);
        }
        return Err(anyhow::anyhow!("mpv salió prematuramente con estado {}", status));
    }

    logger::log_info("player::mpv", "mpv iniciado y reproduciendo en background");

    if let (Some(sock_path), Some(tx)) = (ipc_path.clone(), options.progress_tx.clone()) {
        let slug = options.anime_slug.clone();
        let ep_num = options.episode_number;
        tokio::spawn(async move {
            monitor_mpv_ipc(&sock_path, slug, ep_num, tx).await;
        });
    }

    let cleanup_sock = ipc_path;
    // Evitar procesos zombie al cerrar la ventana de mpv durante la sesión TUI
    tokio::task::spawn_blocking(move || {
        match child.wait() {
            Ok(status) => logger::log_info("player::mpv", &format!("mpv finalizó: {}", status)),
            Err(e) => logger::log_warn("player::mpv", &format!("Error esperando a mpv: {}", e)),
        }
        if let Some(ref p) = cleanup_sock {
            let _ = std::fs::remove_file(p);
        }
    });

    Ok(())
}

#[derive(Debug, PartialEq)]
enum MpvIpcEvent {
    PlaybackRestart,
    EndFileEof,
    TimePos(f64),
    Duration(f64),
}

fn parse_mpv_ipc_line(line: &str) -> Option<MpvIpcEvent> {
    let val: serde_json::Value = serde_json::from_str(line).ok()?;
    let event = val.get("event")?.as_str()?;
    match event {
        "playback-restart" => Some(MpvIpcEvent::PlaybackRestart),
        "end-file" => (val.get("reason").and_then(|r| r.as_str()) == Some("eof"))
            .then_some(MpvIpcEvent::EndFileEof),
        "property-change" => match val.get("name").and_then(|n| n.as_str()) {
            Some("time-pos") => val
                .get("data")
                .and_then(|d| d.as_f64())
                .filter(|s| s.is_finite() && *s >= 0.0)
                .map(MpvIpcEvent::TimePos),
            Some("duration") => val
                .get("data")
                .and_then(|d| d.as_f64())
                .filter(|d| d.is_finite() && *d > 0.0)
                .map(MpvIpcEvent::Duration),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(unix)]
async fn monitor_mpv_ipc(
    sock_path: &Path,
    anime_slug: String,
    episode_number: u32,
    tx: UnboundedSender<ProgressUpdate>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let mut stream_opt = None;
    for _ in 0..25 {
        match UnixStream::connect(sock_path).await {
            Ok(s) => {
                stream_opt = Some(s);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(120)).await,
        }
    }

    let Some(stream) = stream_opt else {
        logger::log_warn("player::ipc", "no se pudo conectar al socket IPC de mpv");
        return;
    };

    let (reader, mut writer) = stream.into_split();
    let subscribe = concat!(
        r#"{"command":["observe_property",1,"time-pos"]}"#,
        "\n",
        r#"{"command":["observe_property",2,"duration"]}"#,
        "\n"
    );
    if writer.write_all(subscribe.as_bytes()).await.is_err() {
        return;
    }

    let mut lines = BufReader::new(reader).lines();
    let mut playback_restarted = false;
    let mut last_pos: Option<u64> = None;
    let mut last_dur: Option<u64> = None;
    let mut last_sent_pos: Option<u64> = None;
    let mut last_sent_at: Option<Instant> = None;
    let mut ended_eof = false;

    while let Ok(Some(line)) = lines.next_line().await {
        match parse_mpv_ipc_line(&line) {
            Some(MpvIpcEvent::PlaybackRestart) => {
                playback_restarted = true;
            }
            Some(MpvIpcEvent::EndFileEof) => {
                ended_eof = true;
            }
            Some(MpvIpcEvent::Duration(d)) => {
                last_dur = Some(d.floor().max(0.0) as u64);
            }
            Some(MpvIpcEvent::TimePos(secs)) => {
                if !playback_restarted {
                    continue;
                }
                let pos = secs.floor().max(0.0) as u64;
                if pos < 3 && last_pos.is_none_or(|p| p < 5) {
                    continue;
                }
                last_pos = Some(pos);

                let should_emit = last_sent_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(4))
                    && last_sent_pos != Some(pos);
                if should_emit {
                    last_sent_at = Some(Instant::now());
                    last_sent_pos = Some(pos);
                    let _ = tx.send(ProgressUpdate {
                        anime_slug: anime_slug.clone(),
                        episode_number,
                        position_secs: pos,
                        duration_secs: last_dur,
                        ended_eof: false,
                    });
                }
            }
            None => {}
        }
    }

    if let Some(pos) = last_pos {
        if last_sent_pos != Some(pos) || ended_eof {
            let _ = tx.send(ProgressUpdate {
                anime_slug,
                episode_number,
                position_secs: pos,
                duration_secs: last_dur,
                ended_eof,
            });
        }
    } else if ended_eof {
        let _ = tx.send(ProgressUpdate {
            anime_slug,
            episode_number,
            position_secs: last_dur.unwrap_or(0),
            duration_secs: last_dur,
            ended_eof: true,
        });
    }
}

#[cfg(not(unix))]
async fn monitor_mpv_ipc(
    _sock_path: &Path,
    _anime_slug: String,
    _episode_number: u32,
    _tx: UnboundedSender<ProgressUpdate>,
) {
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
        let sock = Path::new("/tmp/ani-tui-test.sock");
        let args = build_mpv_args(
            "https://player.zilla-networks.com/m3u8/7601658844858539e438dceee32e7924",
            Some("https://player.zilla-networks.com/play/7601658844858539e438dceee32e7924"),
            &headers,
            Some(872),
            Some(sock),
        )
        .unwrap();

        assert!(args.contains(&"--no-terminal".to_string()));
        assert!(args.contains(&"--really-quiet".to_string()));
        assert!(args.contains(&"--force-window=immediate".to_string()));
        assert!(args.contains(&"--start=872".to_string()));
        assert!(args.contains(&"--input-ipc-server=/tmp/ani-tui-test.sock".to_string()));
        assert!(args.iter().any(|a| a.starts_with("--user-agent=Mozilla/")));
        assert!(args.contains(&"--referrer=https://player.zilla-networks.com/play/7601658844858539e438dceee32e7924".to_string()));
        assert!(args.contains(&"--http-header-fields-append=Origin: https://player.zilla-networks.com".to_string()));
        assert!(args.contains(&"--http-header-fields-append=Sec-Fetch-Site: same-origin".to_string()));
    }

    #[test]
    fn build_mpv_args_skips_tiny_start_and_rejects_bad_scheme() {
        let args = build_mpv_args("https://example.com/v.mp4", None, &[], Some(2), None).unwrap();
        assert!(!args.iter().any(|a| a.starts_with("--start=")));
        assert!(build_mpv_args("file:///etc/passwd", None, &[], None, None).is_err());

        let bad_headers = vec![("Origin".to_string(), "https://ok\r\nEvil: 1".to_string())];
        let args2 = build_mpv_args("https://example.com/v.mp4", None, &bad_headers, None, None).unwrap();
        assert!(!args2.iter().any(|a| a.contains("Evil:")));
    }

    #[test]
    fn parses_real_mpv_ipc_json_events() {
        assert_eq!(
            parse_mpv_ipc_line(r#"{"event":"playback-restart"}"#),
            Some(MpvIpcEvent::PlaybackRestart)
        );
        assert_eq!(
            parse_mpv_ipc_line(r#"{"event":"property-change","id":1,"name":"time-pos","data":872.5}"#),
            Some(MpvIpcEvent::TimePos(872.5))
        );
        assert_eq!(
            parse_mpv_ipc_line(r#"{"event":"property-change","id":2,"name":"duration","data":1440.0}"#),
            Some(MpvIpcEvent::Duration(1440.0))
        );
        assert_eq!(
            parse_mpv_ipc_line(r#"{"event":"end-file","reason":"eof"}"#),
            Some(MpvIpcEvent::EndFileEof)
        );
        assert_eq!(
            parse_mpv_ipc_line(r#"{"event":"end-file","reason":"quit"}"#),
            None
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn monitor_mpv_ipc_streams_progress_and_flushes_on_close() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixListener;

        let sock_path =
            std::env::temp_dir().join(format!("ani_tui_ipc_unit_{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path).unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();
            let _ = lines.next_line().await;
            let _ = lines.next_line().await;

            // Ignorar ruido pre-seek; solo tras playback-restart se actualiza last_pos
            let payload = concat!(
                r#"{"event":"property-change","id":1,"name":"time-pos","data":0.0}"#, "\n",
                r#"{"event":"playback-restart"}"#, "\n",
                r#"{"event":"property-change","id":2,"name":"duration","data":1440.0}"#, "\n",
                r#"{"event":"property-change","id":1,"name":"time-pos","data":872.5}"#, "\n",
                r#"{"event":"end-file","reason":"quit"}"#, "\n"
            );
            writer.write_all(payload.as_bytes()).await.unwrap();
            writer.shutdown().await.unwrap();
        });

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        monitor_mpv_ipc(&sock_path, "naruto".into(), 5, tx).await;
        server.await.unwrap();
        let _ = std::fs::remove_file(&sock_path);

        let update = rx.recv().await.expect("debió reportar último minuto");
        assert_eq!(update.anime_slug, "naruto");
        assert_eq!(update.episode_number, 5);
        assert_eq!(update.position_secs, 872);
        assert_eq!(update.duration_secs, Some(1440));
        assert!(!update.ended_eof);
    }
}

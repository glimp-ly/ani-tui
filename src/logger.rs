// ============================================================
// src/logger.rs — Sistema de logging para ani-tui
// ============================================================
//
// Registra eventos internos de la aplicación en archivos de log
// rotativos en ~/.local/share/ani-tui/logs/.
//
// El logging es especialmente útil para depurar:
//   - Comandos externos (mpv, navegador) y su resultado
//   - URLs que se intentan reproducir
//   - Errores de scraping y red
//   - Eventos de navegación en la TUI
//
// Niveles de log:
//   INFO  — operaciones normales
//   WARN  — situaciones inesperadas pero no fatales
//   ERROR — errores que afectan la experiencia del usuario
//   CMD   — comandos externos ejecutados (mpv, xdg-open)
//   NET   — peticiones HTTP realizadas
//   SEC   — eventos relevantes para seguridad
// ============================================================

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use once_cell::sync::Lazy;

// ————————————————————————————————————————————————
// Estado global del logger
// ————————————————————————————————————————————————

/// Logger global — inicializado una sola vez al arrancar la aplicación.
static LOGGER: Lazy<Mutex<Logger>> = Lazy::new(|| {
    Mutex::new(Logger::new().unwrap_or_else(|e| {
        eprintln!("[ani-tui] Advertencia: no se pudo inicializar el logger: {}", e);
        Logger::null()
    }))
});

// ————————————————————————————————————————————————
// Estructura del logger
// ————————————————————————————————————————————————

/// Logger que escribe en archivo y opcionalmente en stderr.
pub struct Logger {
    /// Ruta del archivo de log activo
    log_path: Option<PathBuf>,
    /// Sesión actual (timestamp de inicio)
    session_id: String,
}

impl Logger {
    /// Crea un nuevo logger con archivo en ~/.local/share/ani-tui/logs/
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let log_dir = get_log_dir()?;
        fs::create_dir_all(&log_dir)?;

        // Nombre del archivo: ani-tui-YYYY-MM-DD.log
        let now = chrono_now();
        let filename = format!("ani-tui-{}.log", &now[..10]); // solo fecha
        let log_path = log_dir.join(&filename);

        // Número de sesión basado en timestamp
        let session_id = now.replace(':', "-").replace(' ', "T");

        let mut logger = Logger {
            log_path: Some(log_path),
            session_id,
        };

        // Escribir encabezado de sesión
        logger.write_raw(&format!(
            "\n╔══════════════════════════════════════╗\n\
             ║  ani-tui — Nueva sesión: {}  ║\n\
             ╚══════════════════════════════════════╝\n",
            &logger.session_id[..19]
        ));

        logger.info("ANI-TUI", &format!("Log inicializado en: {:?}", logger.log_path));
        logger.info("ANI-TUI", &format!("Versión: {}", env!("CARGO_PKG_VERSION")));

        Ok(logger)
    }

    /// Logger nulo (no escribe nada) — fallback si el directorio no es accesible.
    pub fn null() -> Self {
        Logger {
            log_path: None,
            session_id: "null".to_string(),
        }
    }

    /// Escribe un mensaje raw al archivo de log.
    fn write_raw(&mut self, message: &str) {
        if let Some(ref path) = self.log_path {
            if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
                let _ = writeln!(file, "{}", message);
            }
        }
    }

    /// Escribe una línea de log con nivel y timestamp.
    fn log(&mut self, level: &str, module: &str, message: &str) {
        let timestamp = chrono_now();
        let line = format!("[{}] {:5} [{}] {}", timestamp, level, module, message);

        // Siempre al archivo
        if let Some(ref path) = self.log_path {
            if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
                let _ = writeln!(file, "{}", line);
            }
        }
    }

    // — Métodos públicos por nivel —

    /// Log de información general.
    pub fn info(&mut self, module: &str, message: &str) {
        self.log("INFO ", module, message);
    }

    /// Log de advertencia (algo inesperado pero no fatal).
    pub fn warn(&mut self, module: &str, message: &str) {
        self.log("WARN ", module, message);
    }

    /// Log de error (afecta la experiencia del usuario).
    pub fn error(&mut self, module: &str, message: &str) {
        self.log("ERROR", module, message);
    }

    /// Log de comando externo ejecutado.
    /// Registra el programa, argumentos y resultado.
    pub fn cmd(&mut self, program: &str, args: &[&str], result: &str) {
        let cmd_str = format!("{} {}", program, args.join(" "));
        // SEGURIDAD: truncar URLs largas para no llenar el log
        let safe_cmd = sanitize_for_log(&cmd_str, 200);
        self.log("CMD  ", "EXEC", &format!("$ {} → {}", safe_cmd, result));
    }

    /// Log de petición de red.
    pub fn net(&mut self, method: &str, url: &str, status: &str) {
        // SEGURIDAD: no loguear parámetros sensibles de URLs
        let safe_url = sanitize_url_for_log(url);
        self.log("NET  ", "HTTP", &format!("{} {} → {}", method, safe_url, status));
    }

    /// Log de evento de seguridad (validación fallida, URL sospechosa, etc.)
    pub fn security(&mut self, module: &str, message: &str) {
        self.log("SEC  ", module, message);
    }

    /// Devuelve la ruta del archivo de log activo.
    pub fn log_path(&self) -> Option<&PathBuf> {
        self.log_path.as_ref()
    }
}

// ————————————————————————————————————————————————
// Funciones de conveniencia (API pública)
// ————————————————————————————————————————————————

/// Registra un mensaje de INFO en el log global.
pub fn log_info(module: &str, message: &str) {
    if let Ok(mut logger) = LOGGER.lock() {
        logger.info(module, message);
    }
}

/// Registra un mensaje de WARN en el log global.
pub fn log_warn(module: &str, message: &str) {
    if let Ok(mut logger) = LOGGER.lock() {
        logger.warn(module, message);
    }
}

/// Registra un mensaje de ERROR en el log global.
pub fn log_error(module: &str, message: &str) {
    if let Ok(mut logger) = LOGGER.lock() {
        logger.error(module, message);
    }
}

/// Registra un comando externo ejecutado.
pub fn log_cmd(program: &str, args: &[&str], result: &str) {
    if let Ok(mut logger) = LOGGER.lock() {
        logger.cmd(program, args, result);
    }
}

/// Registra una petición HTTP.
pub fn log_net(method: &str, url: &str, status: &str) {
    if let Ok(mut logger) = LOGGER.lock() {
        logger.net(method, url, status);
    }
}

/// Registra un evento de seguridad.
pub fn log_security(module: &str, message: &str) {
    if let Ok(mut logger) = LOGGER.lock() {
        logger.security(module, message);
    }
}

/// Devuelve la ruta del directorio de logs.
pub fn get_log_path() -> Option<PathBuf> {
    LOGGER.lock().ok()?.log_path().cloned()
}

// ————————————————————————————————————————————————
// Helpers internos
// ————————————————————————————————————————————————

/// Obtiene el directorio de logs: ~/.local/share/ani-tui/logs/
fn get_log_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Usar XDG_DATA_HOME si está disponible, sino ~/.local/share
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_home()
                .map(|h| h.join(".local").join("share"))
                .unwrap_or_else(|| PathBuf::from("."))
        });

    Ok(base.join("ani-tui").join("logs"))
}

/// Obtiene el directorio home del usuario actual.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Timestamp actual en formato ISO 8601 simple.
fn chrono_now() -> String {
    // Implementación sin dependencia de chrono
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Conversión manual a fecha/hora UTC
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;

    // Cálculo de año/mes/día desde epoch (simplificado)
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = (day_of_year / 30 + 1).min(12);
    let day = (day_of_year % 30 + 1).min(31);

    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, h, m, s)
}

/// Sanitiza una cadena para el log: trunca si es muy larga.
///
/// SEGURIDAD: Evita que mensajes maliciosos (URLs largas, inyección de newlines)
/// contaminen el archivo de log.
fn sanitize_for_log(s: &str, max_len: usize) -> String {
    // Eliminar newlines y caracteres de control (previene log injection)
    let clean: String = s.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect();

    if clean.len() > max_len {
        format!("{}...[truncado]", &clean[..max_len])
    } else {
        clean
    }
}

/// Sanitiza una URL para el log: elimina parámetros potencialmente sensibles.
///
/// SEGURIDAD: Algunas URLs pueden contener tokens de acceso en el path
/// (como los IDs de Mega o PixelDrain). Se loguea solo el dominio + path base.
fn sanitize_url_for_log(url: &str) -> String {
    // Para URLs de embeds externos, loguear solo dominio + path base
    let sensitive_domains = ["mega.nz", "pixeldrain.com", "1fichier.com"];

    for domain in &sensitive_domains {
        if url.contains(domain) {
            // Extraer solo el dominio
            let proto = if url.starts_with("https") { "https" } else { "http" };
            return format!("{}://{}[...redactado por seguridad]", proto, domain);
        }
    }

    // Para otras URLs, loguear completa pero truncada
    sanitize_for_log(url, 150)
}

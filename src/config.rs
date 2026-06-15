// ============================================================
// src/config.rs — Configuración global del scraper
// ============================================================
//
// Centraliza todas las constantes y parámetros de configuración
// para facilitar el mantenimiento ante cambios en el sitio objetivo.
//
// Si animeav1.com cambia de dominio o estructura, este es el primer
// archivo que se debe actualizar.
// ============================================================

/// URL base del sitio de anime. Cambiar aquí si el dominio cambia.
pub const BASE_URL: &str = "https://animeav1.com";

/// User-Agent que imita Chrome en Windows. Necesario porque el sitio
/// puede devolver contenido diferente o bloqueado con UAs no reconocidos.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Timeout en segundos para las peticiones HTTP.
/// El sitio puede ser lento; 30 segundos es un margen razonable.
pub const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Puerto del servidor API cuando se usa el subcomando `serve`.
pub const SERVE_PORT: u16 = 3030;

/// Dirección de escucha del servidor API.
pub const SERVE_ADDR: &str = "127.0.0.1";

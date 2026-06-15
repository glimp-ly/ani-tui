// ============================================================
// src/security.rs — Validaciones de seguridad centralizadas
// ============================================================
//
// Funciones de validación que previenen ataques comunes:
//
//   SSRF (Server-Side Request Forgery):
//     Cuando ani-tui actúa como servidor API (modo serve), un atacante
//     podría intentar hacer fetch a IPs internas o servicios locales
//     pasando slugs maliciosos como "../../etc/passwd" o
//     "127.0.0.1:8080/internal".
//     → Validar que los slugs solo contengan caracteres permitidos.
//
//   Path Traversal:
//     Un slug con ".." podría escapar del dominio base al construir URLs.
//     → Validar que los slugs no contengan ".." ni "/" ni caracteres especiales.
//
//   Log Injection:
//     Datos del usuario escritos en logs podrían contaminar el formato
//     si contienen newlines o secuencias de escape ANSI.
//     → Sanitizar antes de escribir en logs (ver logger.rs).
//
//   Command Injection:
//     URLs pasadas a mpv o xdg-open como argumentos de proceso
//     (no como strings de shell) ya son seguras por diseño en Rust.
//     Aún así se valida el esquema (solo http/https).
//     → Ver player.rs para las validaciones de URL.
//
//   Open Redirect:
//     Si la aplicación abre URLs del usuario en el navegador,
//     validar que sean http/https y no javascript:, file:, etc.
//     → Ver player.rs.
// ============================================================

use crate::logger;

// ————————————————————————————————————————————————
// Validación de slugs de anime
// ————————————————————————————————————————————————

/// Valida que un slug de anime sea seguro para usar en URLs.
///
/// Un slug válido contiene solo:
/// - Letras minúsculas (a-z)
/// - Números (0-9)
/// - Guiones (-) como separadores de palabras
///
/// Se rechazan:
/// - ".." (path traversal)
/// - "/" y "\" (separadores de directorio)
/// - "%" (URL encoding que podría ofuscar caracteres)
/// - "@" (podría confundirse con user:pass@host)
/// - Espacios y otros caracteres especiales
/// - Slugs demasiado largos (>200 caracteres)
///
/// # Ejemplo de slugs válidos
/// - "one-piece"
/// - "serial-experiments-lain"
/// - "naruto-shippuuden-movie-6-road-to-ninja"
///
/// # Ejemplo de slugs rechazados
/// - "../../etc/passwd"  → path traversal
/// - "127.0.0.1"         → intento de SSRF
/// - "a/b"              → separador de directorio
/// - "abc%2F.."         → URL encoding ofuscado
pub fn validate_anime_slug(slug: &str) -> Result<(), SecurityError> {
    // Longitud máxima razonable
    if slug.is_empty() {
        return Err(SecurityError::InvalidSlug {
            slug: slug.to_string(),
            reason: "slug vacío".to_string(),
        });
    }

    if slug.len() > 200 {
        return Err(SecurityError::InvalidSlug {
            slug: format!("{}...", &slug[..20]),
            reason: format!("slug demasiado largo ({} chars, máximo 200)", slug.len()),
        });
    }

    // Solo permitir letras, números y guiones
    // Nota: slug.chars() itera correctamente sobre Unicode
    for ch in slug.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '-' {
            logger::log_security("security", &format!(
                "Slug rechazado: carácter inválido '{}' en '{}'",
                ch,
                if slug.len() > 30 { &slug[..30] } else { slug }
            ));
            return Err(SecurityError::InvalidSlug {
                slug: slug.to_string(),
                reason: format!("carácter no permitido: '{}'", ch),
            });
        }
    }

    // Verificar que no empiece ni termine con guión
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(SecurityError::InvalidSlug {
            slug: slug.to_string(),
            reason: "el slug no puede empezar ni terminar con guión".to_string(),
        });
    }

    // Verificar que no haya guiones dobles consecutivos (podría ser un intento de bypass)
    if slug.contains("--") {
        return Err(SecurityError::InvalidSlug {
            slug: slug.to_string(),
            reason: "guiones dobles consecutivos no permitidos".to_string(),
        });
    }

    Ok(())
}

/// Valida un número de episodio.
///
/// Los números de episodio son enteros positivos.
/// Se rechaza 0 y números demasiado grandes (>99999).
pub fn validate_episode_number(number: u32) -> Result<(), SecurityError> {
    if number == 0 {
        return Err(SecurityError::InvalidEpisodeNumber {
            number,
            reason: "el número de episodio no puede ser 0".to_string(),
        });
    }

    // El anime con más episodios conocido es Sazae-san con ~7700 episodios
    // Usamos 99999 como límite seguro pero razonable
    if number > 99_999 {
        return Err(SecurityError::InvalidEpisodeNumber {
            number,
            reason: format!("número de episodio demasiado grande: {}", number),
        });
    }

    Ok(())
}

/// Valida una consulta de búsqueda de anime.
///
/// Las consultas de búsqueda pueden contener caracteres más amplios
/// que los slugs, pero se limita la longitud y se rechazan patrones
/// de inyección.
pub fn validate_search_query(query: &str) -> Result<(), SecurityError> {
    if query.trim().is_empty() {
        return Err(SecurityError::InvalidQuery {
            reason: "consulta de búsqueda vacía".to_string(),
        });
    }

    if query.len() > 500 {
        return Err(SecurityError::InvalidQuery {
            reason: format!("consulta demasiado larga ({} chars, máximo 500)", query.len()),
        });
    }

    // Detectar patrones de inyección básicos
    // (las consultas se usan como parámetro GET, no en SQL ni comandos de shell)
    let suspicious_patterns = ["<script", "javascript:", "file://", "data:", "\0"];
    for pattern in &suspicious_patterns {
        if query.to_lowercase().contains(pattern) {
            logger::log_security("security", &format!(
                "Query sospechosa rechazada: contiene patrón '{}' en query='{}'",
                pattern,
                if query.len() > 50 { &query[..50] } else { query }
            ));
            return Err(SecurityError::InvalidQuery {
                reason: format!("patrón no permitido en la consulta: '{}'", pattern),
            });
        }
    }

    Ok(())
}

// ————————————————————————————————————————————————
// Error de seguridad
// ————————————————————————————————————————————————

/// Errores de validación de seguridad.
#[derive(Debug)]
pub enum SecurityError {
    /// Slug de anime con formato inválido o peligroso
    InvalidSlug { slug: String, reason: String },
    /// Número de episodio fuera de rango
    InvalidEpisodeNumber { number: u32, reason: String },
    /// Consulta de búsqueda inválida
    InvalidQuery { reason: String },
}

impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityError::InvalidSlug { slug, reason } => {
                write!(f, "Slug inválido '{}': {}", slug, reason)
            }
            SecurityError::InvalidEpisodeNumber { number, reason } => {
                write!(f, "Número de episodio inválido {}: {}", number, reason)
            }
            SecurityError::InvalidQuery { reason } => {
                write!(f, "Consulta de búsqueda inválida: {}", reason)
            }
        }
    }
}

impl std::error::Error for SecurityError {}

// anyhow convierte cualquier tipo que implemente std::error::Error automáticamente,
// por lo que no se necesita un impl From manual.
// Usar: anyhow::Error::from(security_error) o security_error.into()

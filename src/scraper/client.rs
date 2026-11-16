// ============================================================
// src/scraper/client.rs — Cliente HTTP para scraping
// ============================================================
//
// Crea y configura el cliente HTTP de reqwest para hacer peticiones
// al sitio animeav1.com sin necesidad de navegador headless.
//
// El sitio es un SSR Nuxt.js que envía todos los datos en el HTML
// inicial, por lo que un cliente HTTP simple es suficiente.
// ============================================================

use anyhow::{Context, Result};
use reqwest::Client;
use std::sync::OnceLock;
use std::time::Duration;

use crate::config::{REQUEST_TIMEOUT_SECS, USER_AGENT};
use crate::logger;

/// Instancia única compartida del cliente HTTP para reutilizar el pool
/// de conexiones TCP/TLS y HTTP/2 entre búsquedas, scraping y resolución.
static SHARED_CLIENT: OnceLock<Client> = OnceLock::new();

/// Devuelve una referencia al cliente HTTP compartido, inicializándolo
/// una única vez bajo demanda.
pub fn get_client() -> Result<&'static Client> {
    if let Some(client) = SHARED_CLIENT.get() {
        return Ok(client);
    }

    let client = build_client()?;
    Ok(SHARED_CLIENT.get_or_init(|| client))
}

/// Ejecuta GET validando estado HTTP exitoso con el cliente compartido.
async fn send_get(url: &str, log_action: &str) -> Result<reqwest::Response> {
    let client = get_client()?;
    logger::log_net("GET", url, log_action);

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Error al conectar con: {}", url))?
        .error_for_status()
        .with_context(|| format!("Respuesta HTTP de error para: {}", url))?;

    logger::log_net("GET", url, &response.status().to_string());
    Ok(response)
}

/// Descarga el cuerpo HTML de una URL verificando código HTTP exitoso.
pub async fn fetch_html(url: &str) -> Result<String> {
    let response = send_get(url, "enviando petición HTML...").await?;
    response
        .text()
        .await
        .context("Error al leer respuesta del servidor")
}

/// Descarga bytes binarios (ej. portadas) aplicando límite de tamaño estricto.
pub async fn fetch_bytes(url: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let mut response = send_get(url, "descargando recurso...").await?;

    if let Some(len) = response.content_length() {
        if len > max_bytes as u64 {
            anyhow::bail!("Recurso demasiado grande ({} bytes, máx {})", len, max_bytes);
        }
    }

    let capacity = response
        .content_length()
        .and_then(|l| usize::try_from(l).ok())
        .unwrap_or(0)
        .min(max_bytes);
    let mut buf = Vec::with_capacity(capacity);

    while let Some(chunk) = response
        .chunk()
        .await
        .context("Error al leer descarga del servidor")?
    {
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            anyhow::bail!("Descarga excedió el límite de {} bytes", max_bytes);
        }
        buf.extend_from_slice(&chunk);
    }

    Ok(buf)
}

/// Construye y devuelve un cliente HTTP configurado para scraping.
///
/// Configuración:
/// - User-Agent de Chrome (necesario para evitar bloqueos)
/// - Timeout de 30 segundos
/// - Seguimiento de redirecciones habilitado
/// - Sin caché (siempre datos frescos)
///
/// # Errores
/// Devuelve error si reqwest no puede construir el cliente
/// (situación muy poco frecuente, normalmente solo si el TLS falla).
pub fn build_client() -> Result<Client> {
    let client = Client::builder()
        // User-Agent de Chrome real para evitar bloqueos del sitio
        .user_agent(USER_AGENT)
        // Timeout global para evitar bloqueos indefinidos
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        // Seguir redirecciones HTTP (el sitio puede redirigir slugs)
        .redirect(reqwest::redirect::Policy::limited(5))
        // La descompresión gzip se activa automáticamente si reqwest
        // se compila con la feature "gzip" (habilitada en Cargo.toml)
        .build()
        .map_err(|e| anyhow::anyhow!("Error al construir cliente HTTP: {}", e))?;

    Ok(client)
}

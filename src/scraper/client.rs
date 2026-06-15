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

use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

use crate::config::{REQUEST_TIMEOUT_SECS, USER_AGENT};

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

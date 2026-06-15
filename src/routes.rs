// ============================================================
// src/routes.rs — Rutas de la API REST (modo `serve`)
// ============================================================
//
// Define los endpoints HTTP cuando se ejecuta con `ani-tui serve`.
// Usa el nuevo scraper sin headless Chrome.
//
// Endpoints disponibles:
//   GET /search?q=<query>                → lista de animes
//   GET /anime/:slug/episodes            → episodios de un anime
//   GET /episode/:slug/:number/sources   → fuentes de video
//
// Solo compilado cuando la feature "serve" está activa.
// ============================================================

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::scraper;
use crate::structs::{Anime, Episode, EpisodeSources};

// ————————————————————————————————————————————————
// Parámetros de query
// ————————————————————————————————————————————————

/// Parámetros de la ruta /search
#[derive(Debug, Deserialize)]
pub struct SearchParams {
    /// Término de búsqueda
    q: String,
    /// Página de resultados (opcional, por defecto 1)
    #[serde(default = "default_page")]
    page: u32,
}

fn default_page() -> u32 { 1 }

// ————————————————————————————————————————————————
// Handlers
// ————————————————————————————————————————————————

/// GET /search?q=<query>&page=<page>
/// Busca anime en el catálogo y devuelve la lista en JSON.
pub async fn search_anime(
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<Anime>>, StatusCode> {
    match scraper::search_anime(&params.q, params.page).await {
        Ok(result) => Ok(Json(result.animes)),
        Err(e) => {
            eprintln!("[API] Error en búsqueda '{}': {:?}", params.q, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// GET /anime/:slug/episodes
/// Obtiene la lista de episodios de un anime dado su slug.
pub async fn get_anime_episodes(
    Path(slug): Path<String>,
) -> Result<Json<Vec<Episode>>, StatusCode> {
    match scraper::get_episodes_by_slug(&slug).await {
        Ok((episodes, _metadata)) => Ok(Json(episodes)),
        Err(e) => {
            eprintln!("[API] Error al obtener episodios de '{}': {:?}", slug, e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

/// GET /episode/:slug/:number/sources
/// Obtiene las fuentes de video de un episodio específico.
/// El número de episodio se pasa directamente en la URL.
pub async fn get_episode_sources(
    Path((slug, number)): Path<(String, u32)>,
) -> Result<Json<EpisodeSources>, StatusCode> {
    match scraper::get_video_sources(&slug, number).await {
        Ok(sources) => Ok(Json(sources)),
        Err(e) => {
            eprintln!("[API] Error al obtener fuentes de '{}/{}': {:?}", slug, number, e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

// ————————————————————————————————————————————————
// Router
// ————————————————————————————————————————————————

/// Crea el router de la API REST con todos los endpoints configurados.
pub fn create_routes() -> Router {
    Router::new()
        // Búsqueda de anime: GET /search?q=naruto&page=1
        .route("/search", get(search_anime))
        // Episodios por slug: GET /anime/one-piece/episodes
        .route("/anime/:slug/episodes", get(get_anime_episodes))
        // Fuentes de video: GET /episode/one-piece/1/sources
        .route("/episode/:slug/:number/sources", get(get_episode_sources))
}
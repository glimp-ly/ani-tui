// ============================================================
// src/scraper/mod.rs — Módulo de scraping
// ============================================================
//
// Punto de entrada público del módulo de scraping.
// Re-exporta las funciones principales de cada sub-módulo
// para un acceso más conveniente desde el resto de la aplicación.
//
// Estructura del módulo:
//   scraper::client   - Cliente HTTP configurado
//   scraper::search   - Búsqueda de anime en el catálogo
//   scraper::episodes - Obtención de episodios de un anime
//   scraper::sources  - Obtención de fuentes de video de un episodio
// ============================================================

pub mod client;
pub mod episodes;
pub mod search;
pub mod sources;

// Re-exportaciones convenientes para el uso desde otros módulos
pub use search::search_anime;
pub use episodes::{get_episodes, get_episodes_by_slug};
pub use sources::get_video_sources;

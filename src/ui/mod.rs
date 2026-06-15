// ============================================================
// src/ui/mod.rs — Módulo de interfaz de usuario (TUI)
// ============================================================
//
// Punto de entrada del módulo de UI.
// Exporta la función principal `render` que selecciona qué
// pantalla renderizar según el estado actual de la aplicación.
//
// Estructura del módulo:
//   ui::search   - Pantalla de búsqueda (pantalla inicial)
//   ui::results  - Lista de resultados de búsqueda
//   ui::episodes - Lista de episodios de un anime
//   ui::sources  - Selección de servidor de video
//   ui::help     - Modal de ayuda (overlay)
// ============================================================

pub mod episodes;
pub mod help;
pub mod results;
pub mod search;
pub mod sources;

use ratatui::Frame;

use crate::app::{App, Screen};

/// Función principal de renderizado de la TUI.
///
/// Selecciona y renderiza la pantalla correcta según el estado
/// actual de la aplicación (`app.screen`).
///
/// Esta función se llama en cada ciclo del event loop.
pub fn render(f: &mut Frame, app: &App) {
    match app.screen {
        Screen::Search => search::render_search(f, app),
        Screen::Results => results::render_results(f, app),
        Screen::Episodes => episodes::render_episodes(f, app),
        Screen::Sources => sources::render_sources(f, app),
        Screen::Help => {
            // El Help es un overlay: renderizar la pantalla anterior y superponer el modal
            // Por defecto mostrar la pantalla de búsqueda como fondo
            search::render_search(f, app);
            help::render_help(f, app);
        }
    }
}

// ============================================================
// src/ui/help.rs — Modal de ayuda con keybindings
// ============================================================
//
// Muestra un modal overlay con todos los atajos de teclado
// disponibles en la TUI. Se puede invocar desde cualquier
// pantalla con la tecla '?'.
// ============================================================

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::app::App;
use crate::ui::search::palette;

/// Renderiza el modal de ayuda sobre la pantalla actual.
pub fn render_help(f: &mut Frame, _app: &App) {
    let area = f.area();

    // Calcular área del modal (70% del ancho, 80% del alto)
    let modal_area = centered_rect(70, 85, area);

    // Limpiar el área del modal
    f.render_widget(Clear, modal_area);

    let help_text = build_help_text();

    let modal = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled(" ❓ ", Style::default().fg(palette::ACCENT)),
                    Span::styled(
                        "Ayuda — Atajos de teclado",
                        Style::default()
                            .fg(palette::TEXT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" ", Style::default()),
                ]))
                .title_bottom(Line::from(Span::styled(
                    " Presiona [?] o [Esc] para cerrar ",
                    Style::default().fg(palette::TEXT_DIM),
                )))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(palette::ACCENT))
                .style(Style::default().bg(palette::BG)),
        )
        .alignment(Alignment::Left);

    f.render_widget(modal, modal_area);
}

/// Construye el contenido de texto del modal de ayuda.
fn build_help_text() -> Vec<Line<'static>> {
    vec![
        // — Navegación general —
        section_title("🧭  Navegación General"),
        key_help("↑ / ↓", "Mover selección arriba / abajo en la lista activa"),
        key_help("PgUp / PgDn", "Saltar 10 items hacia arriba / abajo"),
        key_help("Home / End", "Ir al primer / último item de la lista"),
        key_help("Enter", "Confirmar selección (buscar / ver episodios / reproducir)"),
        key_help("Esc / q", "Volver a la pantalla anterior"),
        key_help("?", "Mostrar / ocultar esta ayuda"),
        blank_line(),

        // — Búsqueda —
        section_title("🔍  Búsqueda"),
        key_help("/ (barra)", "Ir a la pantalla de búsqueda desde cualquier lugar"),
        key_help("Backspace", "Borrar el último carácter del campo de búsqueda"),
        key_help("Ctrl+U", "Borrar todo el campo de búsqueda"),
        key_help("Enter", "Ejecutar la búsqueda con el texto actual"),
        blank_line(),

        // — Resultados —
        section_title("📋  Lista de Resultados"),
        key_help("↑ / ↓", "Navegar por la lista de animes encontrados"),
        key_help("→", "Cargar siguiente página de resultados"),
        key_help("←", "Cargar página anterior de resultados"),
        key_help("Enter", "Ver episodios del anime seleccionado"),
        blank_line(),

        // — Episodios —
        section_title("📺  Lista de Episodios"),
        key_help("↑ / ↓", "Navegar por la lista de episodios"),
        key_help("PgUp / PgDn", "Saltar 10 episodios"),
        key_help("Home / End", "Primer / último episodio"),
        key_help("Enter", "Ver fuentes de video del episodio seleccionado"),
        blank_line(),

        // — Fuentes de video —
        section_title("🎬  Fuentes de Video"),
        key_help("↑ / ↓", "Navegar por la lista de servidores"),
        key_help("Tab", "Cambiar entre SUB (subtitulado) y DUB (doblado)"),
        key_help("s", "Cambiar a SUB directamente"),
        key_help("d", "Cambiar a DUB directamente"),
        key_help("Enter", "Reproducir con mpv → navegador (automático)"),
        blank_line(),

        // — Reproductores —
        section_title("▶  Reproducción"),
        key_help("HLS / PDrain", "Compatible con mpv directamente (sin anuncios)"),
        key_help("Mega / MP4Upload", "Se abre en el navegador del sistema"),
        key_help("mpv", "Se lanza en background, la TUI sigue activa"),
    ]
}

/// Crea una línea de título de sección.
fn section_title(title: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            title,
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Crea una línea de atajo de teclado con descripción.
fn key_help(key: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("    "),
        Span::styled(
            format!("{:<18}", key),
            Style::default()
                .fg(palette::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            description,
            Style::default().fg(palette::TEXT_DIM),
        ),
    ])
}

/// Línea en blanco para separación visual.
fn blank_line() -> Line<'static> {
    Line::from("")
}

/// Calcula el área centrada para un widget modal dado el porcentaje de pantalla.
///
/// # Parámetros
/// - `percent_x`: Ancho del modal como porcentaje del ancho de pantalla
/// - `percent_y`: Alto del modal como porcentaje del alto de pantalla
/// - `area`: Área total disponible
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

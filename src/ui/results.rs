// ============================================================
// src/ui/results.rs — Pantalla de resultados de búsqueda
// ============================================================
//
// Muestra la lista de animes encontrados en la búsqueda,
// con navegación con flechas y previsualización de sinopsis.
//
// Layout:
//   ┌─ Resultados: "one piece" (53) ──────────────────────────┐
//   │ > [TV]   One Piece                                ★ 8.7 │
//   │   [TV]   One Piece: Gyojin Tou-hen               ★ 7.9 │
//   │   [OVA]  One Piece Fan Letter                    ★ 7.5 │
//   │   [MOV]  One Piece Film: Red                     ★ 8.1 │
//   │   ...                                                   │
//   ├─────────────────────────────────────────────────────────┤
//   │ Sinopsis del anime seleccionado...                      │
//   │ (texto truncado)                                        │
//   ├─────────────────────────────────────────────────────────┤
//   │ [↑↓] Navegar  [Enter] Ver episodios  [/] Nueva búsqueda │
//   └─────────────────────────────────────────────────────────┘
// ============================================================

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
};

use crate::app::App;
use crate::ui::search::palette;
use crate::ui::{PreviewTarget, category_badge, render_anime_preview};

/// Renderiza la pantalla de resultados de búsqueda.
pub fn render_results(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Fondo
    let bg = Block::default().style(Style::default().bg(palette::BG));
    f.render_widget(bg, area);

    // Layout: contenido principal + barra de atajos
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // lista + portada/detalle
            Constraint::Length(1), // línea de atajos
        ])
        .split(area);

    if area.width >= 78 && area.height >= 16 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(outer[0]);
        render_results_list(f, app, cols[0]);
        render_anime_preview(
            f,
            cols[1],
            app,
            PreviewTarget::SearchResult,
            " Portada y detalle ",
            false,
        );
    } else {
        let preview_h = if area.height >= 22 { 12 } else { 7 };
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(preview_h)])
            .split(outer[0]);
        render_results_list(f, app, rows[0]);
        render_anime_preview(
            f,
            rows[1],
            app,
            PreviewTarget::SearchResult,
            " Portada y detalle ",
            false,
        );
    }

    render_results_keybindings(f, app, outer[1]);

    // Overlay de carga/error/info
    if app.loading.is_loading() {
        if let Some(msg) = app.loading.message() {
            crate::ui::search::render_loading_overlay(f, area, app.spinner_char(), msg);
        }
    }
    if let Some(ref err) = app.error_message {
        crate::ui::search::render_error_overlay(f, area, err);
    }
    if let Some(ref info) = app.info_message {
        crate::ui::search::render_info_overlay(f, area, info);
    }
}

/// Renderiza la lista de resultados de anime.
fn render_results_list(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Título del bloque con query y total
    let query = app.last_query.as_deref().unwrap_or("");
    let total = app.search_results.len();
    let page_info = if app.results_total_pages > 1 {
        format!(" [pág. {}/{}]", app.results_page, app.results_total_pages)
    } else {
        String::new()
    };

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" 🔍 ", Style::default().fg(palette::ACCENT)),
            Span::styled(
                format!("\"{}\" — {} resultados{}", query, total, page_info),
                Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default()),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::BORDER_ACTIVE))
        .style(Style::default().bg(palette::BG));

    // Construir items de la lista en bloque interno para liberar el préstamo sobre app.search_results
    let selected_idx = app.results_state.selected();
    let inner_width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app.search_results
        .iter()
        .enumerate()
        .map(|(i, anime)| {
            let is_selected = selected_idx == Some(i);

            let (cat_text, cat_color) = category_badge(&anime.category);

            // Puntuación (si disponible)
            let score_str = anime.score
                .map(|s| format!(" ★ {:.1}", s))
                .unwrap_or_default();
            let max_title = inner_width
                .saturating_sub(10 + score_str.chars().count())
                .max(12);

            // Construir la línea del item
            let prefix = if is_selected { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default()
                        .fg(if is_selected { palette::HIGHLIGHT } else { palette::BG }),
                ),
                Span::styled(
                    format!("[{}]", cat_text),
                    Style::default().fg(cat_color).add_modifier(Modifier::DIM),
                ),
                Span::raw(" "),
                Span::styled(
                    truncate_str(&anime.title, max_title),
                    Style::default()
                        .fg(if is_selected { palette::TEXT } else { palette::TEXT_DIM })
                        .add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(
                    score_str,
                    Style::default().fg(palette::HIGHLIGHT),
                ),
            ]);

            let style = if is_selected {
                Style::default().bg(ratatui::style::Color::Rgb(20, 30, 50))
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_stateful_widget(list, area, &mut app.results_state);
}

/// Renderiza la barra de atajos de teclado para la pantalla de resultados.
fn render_results_keybindings(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use crate::ui::search::key_span;

    let has_prev = app.results_page > 1;
    let has_next = app.results_page < app.results_total_pages;

    let mut spans = vec![
        key_span("↑↓"), Span::raw(" navegar   "),
        key_span("Enter"), Span::raw(" ver episodios   "),
        key_span("/"), Span::raw(" nueva búsqueda   "),
        key_span("Esc"), Span::raw(" volver   "),
    ];

    if has_prev {
        spans.push(key_span("←"));
        spans.push(Span::raw(" anterior   "));
    }
    if has_next {
        spans.push(key_span("→"));
        spans.push(Span::raw(" siguiente   "));
    }

    let keybindings = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .style(Style::default().fg(palette::TEXT_DIM));

    f.render_widget(keybindings, area);
}

/// Trunca un string a la longitud máxima indicada, añadiendo "…" si es necesario.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

// ============================================================
// src/ui/episodes.rs — Pantalla de lista de episodios
// ============================================================
//
// Muestra la lista de episodios del anime seleccionado.
// Soporta animes con muchos episodios (One Piece: 1166+).
//
// Layout:
//   ┌─ One Piece — 1166 episodios ★ 8.7 ─────────────────────┐
//   │ > Ep. 0001                                              │
//   │   Ep. 0002                                              │
//   │   Ep. 0003                                              │
//   │   ...                                                   │
//   │   Ep. 1166                                              │
//   ├─────────────────────────────────────────────────────────┤
//   │ [↑↓] Navegar  [Enter] Ver fuentes  [g] Ir a Ep.        │
//   │ [Home/End] Inicio/Fin  [Esc] Volver                     │
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
use crate::ui::{PreviewTarget, render_anime_preview};

/// Renderiza la pantalla de lista de episodios.
pub fn render_episodes(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Fondo
    let bg = Block::default().style(Style::default().bg(palette::BG));
    f.render_widget(bg, area);

    // Layout: lista + portada/detalle + barra de atajos
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),     // lista de episodios y ficha del anime
            Constraint::Length(2),  // barra de atajos
        ])
        .split(area);

    if area.width >= 78 && area.height >= 16 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
            .split(layout[0]);
        render_episodes_list(f, app, cols[0]);
        render_anime_preview(
            f,
            cols[1],
            app,
            PreviewTarget::SelectedAnime,
            " Anime seleccionado ",
            false,
        );
    } else if area.height >= 18 {
        let preview_h = if area.height >= 24 { 12 } else { 8 };
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(preview_h), Constraint::Min(6)])
            .split(layout[0]);
        render_anime_preview(
            f,
            rows[0],
            app,
            PreviewTarget::SelectedAnime,
            " Anime seleccionado ",
            false,
        );
        render_episodes_list(f, app, rows[1]);
    } else {
        render_episodes_list(f, app, layout[0]);
    }

    render_episodes_keybindings(f, app, layout[1]);

    // Overlays
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

/// Renderiza la lista de episodios con selección activa.
fn render_episodes_list(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let (anime_title, count_opt, score_opt) = match &app.selected_anime {
        Some(a) => (a.title.clone(), a.episodes_count, a.score),
        None => return,
    };

    // Construir título del bloque
    let count_str = count_opt
        .map(|c| format!("{} episodios", c))
        .unwrap_or_else(|| format!("{} episodios", app.episodes.len()));

    let score_str = score_opt
        .map(|s| format!(" ★ {:.1}", s))
        .unwrap_or_default();

    let selected_idx = app.episodes_state.selected();
    let selected_label = selected_idx
        .and_then(|i| app.episodes.get(i))
        .map(|e| e.display_title())
        .unwrap_or_else(|| "Ninguno".to_string());

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" 📺 ", Style::default().fg(palette::ACCENT)),
            Span::styled(
                anime_title,
                Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" — {} {}", count_str, score_str),
                Style::default().fg(palette::TEXT_DIM),
            ),
            Span::styled(" ", Style::default()),
        ]))
        .title_bottom(Line::from(Span::styled(
            format!(" Seleccionado: {} ", selected_label),
            Style::default().fg(palette::TEXT_DIM),
        )).alignment(Alignment::Right))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::BORDER_ACTIVE))
        .style(Style::default().bg(palette::BG));

    // Construir items de la lista
    let items: Vec<ListItem> = app.episodes
        .iter()
        .enumerate()
        .map(|(i, episode)| {
            let is_selected = selected_idx == Some(i);

            let prefix = if is_selected { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(
                        if is_selected { palette::HIGHLIGHT } else { palette::BG }
                    ),
                ),
                Span::styled(
                    format!("Ep. {:04}", episode.number),
                    Style::default()
                        .fg(palette::ACCENT)
                        .add_modifier(if is_selected { Modifier::BOLD } else { Modifier::DIM }),
                ),
            ]);

            let style = if is_selected {
                Style::default().bg(ratatui::style::Color::Rgb(15, 25, 50))
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_stateful_widget(list, area, &mut app.episodes_state);
}

/// Renderiza los atajos de teclado para la pantalla de episodios.
fn render_episodes_keybindings(f: &mut Frame, _app: &App, area: ratatui::layout::Rect) {
    use crate::ui::search::key_span;

    let line = Line::from(vec![
        key_span("↑↓"), Span::raw(" navegar   "),
        key_span("PgUp/PgDn"), Span::raw(" página   "),
        key_span("Home/End"), Span::raw(" inicio/fin   "),
        key_span("Enter"), Span::raw(" ver fuentes   "),
        key_span("Esc"), Span::raw(" volver"),
    ]);

    let keybindings = Paragraph::new(line)
        .alignment(Alignment::Center)
        .style(Style::default().fg(palette::TEXT_DIM));

    f.render_widget(keybindings, area);
}

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

/// Renderiza la pantalla de lista de episodios.
pub fn render_episodes(f: &mut Frame, app: &App) {
    let area = f.area();

    // Fondo
    let bg = Block::default().style(Style::default().bg(palette::BG));
    f.render_widget(bg, area);

    // Layout: encabezado + lista + barra de atajos
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),     // lista de episodios
            Constraint::Length(2),  // barra de atajos
        ])
        .split(area);

    render_episodes_list(f, app, layout[0]);
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
}

/// Renderiza la lista de episodios con selección activa.
fn render_episodes_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let anime = match &app.selected_anime {
        Some(a) => a,
        None => return,
    };

    // Construir título del bloque
    let count_str = anime.episodes_count
        .map(|c| format!("{} episodios", c))
        .unwrap_or_else(|| format!("{} episodios", app.episodes.len()));

    let score_str = anime.score
        .map(|s| format!(" ★ {:.1}", s))
        .unwrap_or_default();

    let selected_num = app.episodes_state
        .selected()
        .and_then(|i| app.episodes.get(i))
        .map(|e| e.number)
        .unwrap_or(0);

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" 📺 ", Style::default().fg(palette::ACCENT)),
            Span::styled(
                &anime.title,
                Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" — {} {}", count_str, score_str),
                Style::default().fg(palette::TEXT_DIM),
            ),
            Span::styled(" ", Style::default()),
        ]))
        .title_bottom(Line::from(Span::styled(
            format!(" Episodio seleccionado: {} ", selected_num),
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
            let is_selected = app.episodes_state.selected() == Some(i);

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
                        .fg(if is_selected { palette::ACCENT } else { palette::ACCENT })
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
    let mut state = app.episodes_state.clone();
    f.render_stateful_widget(list, area, &mut state);
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

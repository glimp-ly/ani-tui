// ============================================================
// src/ui/sources.rs — Pantalla de selección de fuentes de video
// ============================================================
//
// Muestra las fuentes de video disponibles para el episodio
// seleccionado, organizadas por tipo de audio (SUB/DUB).
// Permite seleccionar un servidor específico y reproducirlo.
//
// Layout:
//   ┌─ Fuentes: One Piece — Episodio 1 ──────────────────────┐
//   │  [SUB] ←→ [DUB]    (tab para cambiar)                  │
//   ├─────────────────────────────────────────────────────────┤
//   │  > [🎬 mpv] HLS       player.zilla-networks.com/...    │
//   │    [🌐 web] Mega      mega.nz/embed/...                 │
//   │    [🌐 web] MP4Upload www.mp4upload.com/embed-...       │
//   │    [🌐 web] UPNShare  animeav1.uns.bio/#...             │
//   ├─────────────────────────────────────────────────────────┤
//   │  [↑↓] Navegar  [Enter] Reproducir  [Tab] SUB/DUB       │
//   │  [Esc] Volver  [c] Copiar URL                          │
//   └─────────────────────────────────────────────────────────┘
// ============================================================

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
};

use crate::app::App;
use crate::structs::AudioType;
use crate::ui::search::palette;

/// Renderiza la pantalla de selección de fuentes de video.
pub fn render_sources(f: &mut Frame, app: &App) {
    let area = f.area();

    // Fondo
    let bg = Block::default().style(Style::default().bg(palette::BG));
    f.render_widget(bg, area);

    // Layout: tabs audio + lista de servidores + info del seleccionado + atajos
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tabs SUB / DUB
            Constraint::Min(6),     // lista de servidores
            Constraint::Length(3),  // info de la fuente seleccionada
            Constraint::Length(2),  // barra de atajos
        ])
        .split(area);

    render_audio_tabs(f, app, layout[0]);
    render_servers_list(f, app, layout[1]);
    render_source_info(f, app, layout[2]);
    render_sources_keybindings(f, layout[3]);

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

/// Renderiza los tabs de selección SUB / DUB.
fn render_audio_tabs(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let anime_title = app.selected_anime.as_ref()
        .map(|a| a.title.as_str())
        .unwrap_or("Anime");

    let ep_num = app.selected_episode.as_ref()
        .map(|e| e.number)
        .unwrap_or(0);

    // Tabs izquierdos: SUB y DUB
    let sub_count = app.sources.sub.len();
    let dub_count = app.sources.dub.len();
    let has_dub = dub_count > 0;

    let sub_style = if app.selected_audio == AudioType::Sub {
        Style::default()
            .fg(palette::SUB_COLOR)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(palette::TEXT_DIM)
    };

    let dub_style = if app.selected_audio == AudioType::Dub {
        Style::default()
            .fg(palette::DUB_COLOR)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else if has_dub {
        Style::default().fg(palette::TEXT_DIM)
    } else {
        Style::default().fg(Color::Rgb(50, 50, 60)) // gris muy oscuro si no hay DUB
    };

    let header = Line::from(vec![
        Span::styled(" 🎬 ", Style::default().fg(palette::ACCENT)),
        Span::styled(
            format!("{} — Ep. {}", anime_title, ep_num),
            Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            format!("[ SUB ({}) ]", sub_count),
            sub_style,
        ),
        Span::raw("  "),
        Span::styled(
            if has_dub {
                format!("[ DUB ({}) ]", dub_count)
            } else {
                "[ DUB (no disponible) ]".to_string()
            },
            dub_style,
        ),
        Span::styled(
            if has_dub { "  ← Tab para cambiar" } else { "" },
            Style::default()
                .fg(palette::TEXT_DIM)
                .add_modifier(Modifier::ITALIC),
        ),
    ]);

    let tabs_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::BORDER))
        .style(Style::default().bg(palette::BG));

    let tabs_widget = Paragraph::new(header).block(tabs_block);
    f.render_widget(tabs_widget, area);
}

/// Renderiza la lista de servidores de video disponibles.
fn render_servers_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let sources = app.sources.get(&app.selected_audio);

    let audio_label = app.selected_audio.display();
    let audio_color = match app.selected_audio {
        AudioType::Sub => palette::SUB_COLOR,
        AudioType::Dub => palette::DUB_COLOR,
    };

    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                format!("{} ", audio_label),
                Style::default().fg(audio_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "— Selecciona un servidor ",
                Style::default().fg(palette::TEXT_DIM),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::BORDER_ACTIVE))
        .style(Style::default().bg(palette::BG));

    if sources.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  No hay fuentes disponibles para este tipo de audio",
            Style::default()
                .fg(palette::TEXT_DIM)
                .add_modifier(Modifier::ITALIC),
        )))
        .block(block);
        f.render_widget(empty, area);
        return;
    }

    // Determinar qué servidores son compatibles con mpv
    let mpv_available = app.player_info.mpv_available;

    let items: Vec<ListItem> = sources
        .iter()
        .enumerate()
        .map(|(i, source)| {
            let is_selected = app.sources_state.selected() == Some(i);

            // Indicador de compatibilidad con mpv o navegador
            let (player_icon, player_color) = if source.is_mpv_compatible() && mpv_available {
                ("🎬 mpv", palette::SUCCESS)
            } else if source.is_mpv_compatible() {
                ("🌐 web", palette::WARNING) // mpv compatible pero no instalado
            } else {
                ("🌐 web", palette::ACCENT2)
            };

            // URL truncada para mostrar
            let url_display = truncate_url(&source.url, 55);
            let prefix = if is_selected { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(if is_selected { palette::HIGHLIGHT } else { palette::BG }),
                ),
                Span::styled(
                    format!("[{}]", player_icon),
                    Style::default().fg(player_color).add_modifier(Modifier::DIM),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:<12}", &source.server),
                    Style::default()
                        .fg(if is_selected { palette::HIGHLIGHT } else { palette::ACCENT })
                        .add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::raw(" "),
                Span::styled(
                    url_display,
                    Style::default().fg(palette::TEXT_DIM),
                ),
            ]);

            let style = if is_selected {
                Style::default().bg(Color::Rgb(15, 30, 50))
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    let mut state = app.sources_state.clone();
    f.render_stateful_widget(list, area, &mut state);
}

/// Renderiza información detallada sobre la fuente seleccionada.
fn render_source_info(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let sources = app.sources.get(&app.selected_audio);
    let selected_source = app.sources_state
        .selected()
        .and_then(|i| sources.get(i));

    let content = if let Some(source) = selected_source {
        let (compat_text, compat_color) = if source.is_mpv_compatible() && app.player_info.mpv_available {
            ("mpv directo — sin anuncios", palette::SUCCESS)
        } else if source.is_mpv_compatible() {
            ("compatible con mpv (no instalado) — se usará navegador", palette::WARNING)
        } else {
            ("se abrirá en el navegador del sistema", palette::ACCENT2)
        };

        vec![Line::from(vec![
            Span::styled("  🔗 ", Style::default().fg(palette::TEXT_DIM)),
            Span::styled(&source.url, Style::default().fg(palette::TEXT_DIM)),
        ]),
        Line::from(vec![
            Span::styled("  📌 ", Style::default().fg(palette::TEXT_DIM)),
            Span::styled(
                format!("Servidor: {}  |  Reproducción: {}", source.server, compat_text),
                Style::default().fg(compat_color),
            ),
        ])]
    } else {
        vec![Line::from(Span::styled(
            "  Selecciona un servidor para ver la URL",
            Style::default().fg(palette::TEXT_DIM).add_modifier(Modifier::ITALIC),
        ))]
    };

    let info = Paragraph::new(content)
        .block(
            Block::default()
                .title(Line::from(Span::styled(
                    " ℹ Info ",
                    Style::default().fg(palette::TEXT_DIM),
                )))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(palette::BORDER))
                .style(Style::default().bg(palette::BG)),
        );

    f.render_widget(info, area);
}

/// Renderiza la barra de atajos de teclado.
fn render_sources_keybindings(f: &mut Frame, area: ratatui::layout::Rect) {
    use crate::ui::search::key_span;

    let line = Line::from(vec![
        key_span("↑↓"), Span::raw(" navegar   "),
        key_span("Enter"), Span::raw(" reproducir   "),
        key_span("Tab"), Span::raw(" SUB/DUB   "),
        key_span("Esc"), Span::raw(" volver"),
    ]);

    let keybindings = Paragraph::new(line)
        .alignment(Alignment::Center)
        .style(Style::default().fg(palette::TEXT_DIM));

    f.render_widget(keybindings, area);
}

/// Trunca una URL para mostrarla en la lista, preservando el dominio.
fn truncate_url(url: &str, max_len: usize) -> String {
    if url.len() <= max_len {
        return url.to_string();
    }

    // Intentar mostrar dominio + inicio + "..."
    let without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    if without_scheme.len() <= max_len {
        return without_scheme.to_string();
    }

    format!("{}…", &without_scheme[..max_len - 1])
}

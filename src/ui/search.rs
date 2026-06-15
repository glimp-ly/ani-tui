// ============================================================
// src/ui/search.rs — Pantalla de búsqueda
// ============================================================
//
// Renderiza la pantalla inicial de la TUI donde el usuario
// puede escribir el nombre del anime que desea buscar.
//
// Layout:
//   ┌─────────────────────────────┐
//   │         ANI-TUI             │  ← título con ASCII art
//   │   🎌 Buscar Anime           │  ← subtítulo
//   │                             │
//   │  ┌─ Buscar ───────────────┐ │  ← campo de input
//   │  │ > one piece_           │ │
//   │  └────────────────────────┘ │
//   │                             │
//   │  [Enter] Buscar  [?] Ayuda  │  ← atajos de teclado
//   │  [Esc] Salir                │
//   └─────────────────────────────┘
// ============================================================

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::app::App;

/// Paleta de colores de la TUI (estilo cyberpunk/oscuro)
pub mod palette {
    use ratatui::style::Color;

    pub const BG: Color = Color::Rgb(10, 10, 20);           // fondo muy oscuro
    pub const ACCENT: Color = Color::Rgb(100, 200, 255);     // azul cyan brillante
    pub const ACCENT2: Color = Color::Rgb(180, 100, 255);    // violeta
    pub const TEXT: Color = Color::Rgb(220, 220, 240);       // blanco suave
    pub const TEXT_DIM: Color = Color::Rgb(120, 120, 140);   // gris claro
    pub const SUCCESS: Color = Color::Rgb(80, 220, 120);     // verde
    pub const ERROR: Color = Color::Rgb(255, 80, 80);        // rojo
    pub const WARNING: Color = Color::Rgb(255, 200, 80);     // amarillo
    pub const HIGHLIGHT: Color = Color::Rgb(255, 180, 50);   // dorado
    pub const BORDER: Color = Color::Rgb(60, 60, 100);       // borde oscuro
    pub const BORDER_ACTIVE: Color = Color::Rgb(100, 200, 255); // borde activo = accent
    pub const SUB_COLOR: Color = Color::Rgb(80, 200, 255);   // azul para SUB
    pub const DUB_COLOR: Color = Color::Rgb(255, 140, 80);   // naranja para DUB
}

/// ASCII art del logo de la aplicación
const LOGO: &str = r"
  ___  _   _ ___ _____ _   _ ___ 
 / _ \| \ | |_ _|_   _| | | |_ _|
| | | |  \| || |  | | | | | || | 
| |_| | |\  || |  | | | |_| || | 
 \__,_|_| \_|___| |_|  \___/|___|
";

/// Renderiza la pantalla de búsqueda completa.
pub fn render_search(f: &mut Frame, app: &App) {
    let area = f.area();

    // Fondo
    let bg = Block::default().style(Style::default().bg(palette::BG));
    f.render_widget(bg, area);

    // Layout principal: centrar verticalmente
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(15), // espacio superior
            Constraint::Length(7),      // logo ASCII
            Constraint::Length(1),      // subtítulo
            Constraint::Length(2),      // separador
            Constraint::Length(3),      // campo de búsqueda
            Constraint::Length(2),      // separador
            Constraint::Length(4),      // atajos de teclado
            Constraint::Min(0),         // espacio inferior
        ])
        .split(area);

    // — Logo ASCII —
    render_logo(f, vertical[1]);

    // — Subtítulo —
    let subtitle = Paragraph::new(Line::from(vec![
        Span::styled("🎌 ", Style::default()),
        Span::styled(
            "Busca y reproduce anime sin anuncios",
            Style::default()
                .fg(palette::TEXT_DIM)
                .add_modifier(Modifier::ITALIC),
        ),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(subtitle, vertical[2]);

    // — Campo de búsqueda (centrado horizontalmente) —
    let h_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ])
        .split(vertical[4]);

    render_search_input(f, app, h_layout[1]);

    // — Estado del reproductor —
    let player_line = if app.player_info.mpv_available {
        Line::from(vec![
            Span::styled("●  ", Style::default().fg(palette::SUCCESS)),
            Span::styled("mpv disponible — reproducción directa sin anuncios",
                Style::default().fg(palette::TEXT_DIM)),
        ])
    } else {
        Line::from(vec![
            Span::styled("●  ", Style::default().fg(palette::WARNING)),
            Span::styled("mpv no encontrado — se usará el navegador del sistema",
                Style::default().fg(palette::TEXT_DIM)),
        ])
    };

    let player_status = Paragraph::new(player_line).alignment(Alignment::Center);
    f.render_widget(player_status, vertical[5]);

    // — Atajos de teclado —
    render_keybindings_search(f, vertical[6]);

    // — Mensaje de error (si hay) —
    if let Some(ref err) = app.error_message {
        render_error_overlay(f, area, err);
    }

    // — Indicador de carga —
    if app.loading.is_loading() {
        if let Some(msg) = app.loading.message() {
            render_loading_overlay(f, area, app.spinner_char(), msg);
        }
    }
}

/// Renderiza el logo ASCII con degradado de colores.
fn render_logo(f: &mut Frame, area: Rect) {
    let logo_lines: Vec<Line> = LOGO
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let color = match i % 5 {
                0 => palette::ACCENT,
                1 => Color::Rgb(80, 180, 255),
                2 => palette::ACCENT2,
                3 => Color::Rgb(150, 80, 255),
                _ => palette::ACCENT,
            };
            Line::from(Span::styled(
                line,
                Style::default()
                    .fg(color)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    let logo = Paragraph::new(logo_lines).alignment(Alignment::Center);
    f.render_widget(logo, area);
}

/// Renderiza el campo de input de búsqueda con el cursor visible.
fn render_search_input(f: &mut Frame, app: &App, area: Rect) {
    let is_active = !app.loading.is_loading();

    let border_color = if is_active {
        palette::BORDER_ACTIVE
    } else {
        palette::BORDER
    };

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" 🔍 ", Style::default().fg(palette::ACCENT)),
            Span::styled("Buscar anime",
                Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(palette::BG));

    // Mostrar input con cursor
    let display_text = format!("{}_", &app.search_input);
    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(palette::ACCENT)),
        Span::styled(
            &app.search_input,
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if is_active { "█" } else { "" },
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]))
    .block(block);

    let _ = display_text; // usado para debug
    f.render_widget(input, area);
}

/// Renderiza los atajos de teclado de la pantalla de búsqueda.
fn render_keybindings_search(f: &mut Frame, area: Rect) {
    let keys = vec![
        Line::from(vec![
            key_span("Enter"), Span::raw(" buscar   "),
            key_span("Esc"),   Span::raw(" salir   "),
            key_span("?"),     Span::raw(" ayuda"),
        ]),
    ];

    let keybindings = Paragraph::new(keys)
        .alignment(Alignment::Center)
        .style(Style::default().fg(palette::TEXT_DIM));

    f.render_widget(keybindings, area);
}

/// Crea un span estilizado para mostrar una tecla de atajo.
pub fn key_span(key: &str) -> Span<'static> {
    Span::styled(
        format!("[{}]", key),
        Style::default()
            .fg(palette::HIGHLIGHT)
            .add_modifier(Modifier::BOLD),
    )
}

/// Renderiza un overlay de carga sobre la pantalla actual.
pub fn render_loading_overlay(f: &mut Frame, area: Rect, spinner: char, msg: &str) {
    // Calcular área centrada para el overlay
    let width = (msg.len() as u16 + 8).min(area.width - 4);
    let height = 3u16;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;

    let overlay_area = Rect::new(area.x + x, area.y + y, width, height);

    f.render_widget(Clear, overlay_area);

    let loading = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {} ", spinner),
            Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(msg, Style::default().fg(palette::TEXT)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(palette::ACCENT))
            .style(Style::default().bg(palette::BG)),
    )
    .alignment(Alignment::Center);

    f.render_widget(loading, overlay_area);
}

/// Renderiza un overlay de error sobre la pantalla actual.
pub fn render_error_overlay(f: &mut Frame, area: Rect, message: &str) {
    let lines: Vec<&str> = message.lines().collect();
    let max_width = lines.iter().map(|l| l.len()).max().unwrap_or(20);
    let width = (max_width as u16 + 6).min(area.width - 4).max(30);
    let height = (lines.len() as u16 + 4).min(area.height - 4);
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;

    let overlay_area = Rect::new(area.x + x, area.y + y, width, height);
    f.render_widget(Clear, overlay_area);

    let error_text: Vec<Line> = std::iter::once(
        Line::from(Span::styled(
            "⚠ Error",
            Style::default()
                .fg(palette::ERROR)
                .add_modifier(Modifier::BOLD),
        ))
    )
    .chain(
        lines.iter().map(|l| {
            Line::from(Span::styled(*l, Style::default().fg(palette::TEXT)))
        })
    )
    .chain(std::iter::once(
        Line::from(Span::styled(
            "Presiona cualquier tecla para continuar",
            Style::default()
                .fg(palette::TEXT_DIM)
                .add_modifier(Modifier::ITALIC),
        ))
    ))
    .collect();

    let error_widget = Paragraph::new(error_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(palette::ERROR))
                .style(Style::default().bg(palette::BG)),
        )
        .alignment(Alignment::Center);

    f.render_widget(error_widget, overlay_area);
}

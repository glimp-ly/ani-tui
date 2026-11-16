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

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};
use ratatui_image::{FilterType, Resize, StatefulImage};

use crate::app::{App, CoverDisplayState, Screen};
use crate::structs::{Anime, AnimeCategory};
use crate::ui::search::palette;

/// Origen de los datos de anime mostrados en el panel de detalle con portada.
#[derive(Clone, Copy)]
pub enum PreviewTarget {
    SearchResult,
    SelectedAnime,
}

/// Función principal de renderizado de la TUI.
///
/// Selecciona y renderiza la pantalla correcta según el estado
/// actual de la aplicación (`app.screen`).
///
/// Esta función se llama en cada ciclo del event loop.
pub fn render(f: &mut Frame, app: &mut App) {
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

/// Etiqueta corta y color de categoría compartidos entre listas y panel de detalle.
pub fn category_badge(category: &AnimeCategory) -> (&'static str, Color) {
    match category {
        AnimeCategory::TvAnime => (" TV  ", palette::ACCENT),
        AnimeCategory::Movie => ("MOV  ", palette::ACCENT2),
        AnimeCategory::Ova => (" OVA ", palette::SUCCESS),
        AnimeCategory::Special => (" ESP ", palette::WARNING),
        AnimeCategory::Ona => (" ONA ", palette::HIGHLIGHT),
        AnimeCategory::Unknown(_) => ("  ?  ", palette::TEXT_DIM),
    }
}

/// Renderiza la tarjeta del anime con la portada junto al nombre, metadatos y sinopsis.
pub fn render_anime_preview(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    target: PreviewTarget,
    block_title: &str,
    active: bool,
) {
    let border_color = if active {
        palette::BORDER_ACTIVE
    } else {
        palette::BORDER
    };

    let block = Block::default()
        .title(Line::from(Span::styled(
            block_title,
            Style::default()
                .fg(if active { palette::TEXT } else { palette::TEXT_DIM })
                .add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(palette::BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width < 10 || inner.height < 4 {
        return;
    }

    let snapshot = {
        let anime: Option<&Anime> = match target {
            PreviewTarget::SearchResult => app
                .results_state
                .selected()
                .and_then(|i| app.search_results.get(i)),
            PreviewTarget::SelectedAnime => app.selected_anime.as_ref(),
        };
        anime.map(|a| {
            let (cat_badge, cat_color) = category_badge(&a.category);
            (
                a.title.clone(),
                cat_badge,
                cat_color,
                a.score,
                a.episodes_count,
                a.synopsis.clone(),
                a.cover_url.clone(),
            )
        })
    };

    let Some((title, cat_badge, cat_color, score, episodes_count, synopsis, cover_url)) = snapshot
    else {
        let empty = Paragraph::new(Line::from(Span::styled(
            "Selecciona un anime para ver su portada y detalles",
            Style::default()
                .fg(palette::TEXT_DIM)
                .add_modifier(Modifier::ITALIC),
        )))
        .wrap(Wrap { trim: true });
        f.render_widget(empty, inner);
        return;
    };

    let show_cover = inner.width >= 26 && inner.height >= 7;
    if !show_cover {
        render_compact_preview(
            f,
            inner,
            &title,
            cat_badge,
            cat_color,
            score,
            episodes_count,
            &synopsis,
        );
        return;
    }

    let header_h = if inner.height >= 14 {
        (inner.height * 55 / 100).clamp(9, 16)
    } else {
        inner.height
    };

    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_h), Constraint::Min(0)])
        .split(inner);

    let (fw, fh) = app.cover_picker.font_size();
    let fw = u32::from(fw.max(1));
    let fh = u32::from(fh.max(1));
    let ideal_w = ((u32::from(header_h) * fh * 300) / (425 * fw)).max(10) as u16;
    let max_w = (inner.width / 2).max(10);
    let cover_w = ideal_w.min(max_w).min(28);

    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(cover_w),
            Constraint::Length(1),
            Constraint::Min(10),
        ])
        .split(v_chunks[0]);

    render_cover_slot(f, h_chunks[0], app, cover_url.as_deref());
    render_title_and_meta(
        f,
        h_chunks[2],
        &title,
        cat_badge,
        cat_color,
        score,
        episodes_count,
    );

    if v_chunks[1].height >= 3 {
        let syn_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(v_chunks[1]);

        let divider = Paragraph::new(Line::from(Span::styled(
            "─ Sinopsis",
            Style::default().fg(palette::BORDER),
        )));
        f.render_widget(divider, syn_chunks[0]);

        let body = if synopsis.is_empty() {
            "Sin sinopsis disponible."
        } else {
            &synopsis
        };
        let syn_widget = Paragraph::new(body)
            .style(Style::default().fg(palette::TEXT_DIM))
            .wrap(Wrap { trim: true });
        f.render_widget(syn_widget, syn_chunks[1]);
    }
}

fn render_cover_slot(f: &mut Frame, area: Rect, app: &mut App, cover_url: Option<&str>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let state = app.cover_state(cover_url);
    let suppress = app.should_suppress_cover_for_overlay();

    match state {
        CoverDisplayState::Ready { frame_encoded } => {
            if !frame_encoded {
                render_centered_label(f, area, "Cargando...");
            }
            if !suppress {
                f.render_stateful_widget(
                    StatefulImage::default().resize(Resize::Fit(Some(FilterType::Triangle))),
                    area,
                    &mut app.cover_protocol,
                );
            }
        }
        CoverDisplayState::Loading => {
            let msg = format!("{} Portada", app.spinner_char());
            render_centered_label(f, area, &msg);
        }
        CoverDisplayState::Unavailable => {
            render_centered_label(f, area, "[Sin portada]");
        }
    }
}

fn render_centered_label(f: &mut Frame, area: Rect, text: &str) {
    let top_pad = area.height / 2;
    let centered = Rect::new(
        area.x,
        area.y.saturating_add(top_pad),
        area.width,
        area.height.saturating_sub(top_pad).min(1),
    );
    let p = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(palette::TEXT_DIM),
    )))
    .alignment(Alignment::Center);
    f.render_widget(p, centered);
}

fn render_title_and_meta(
    f: &mut Frame,
    area: Rect,
    title: &str,
    cat_badge: &str,
    cat_color: Color,
    score: Option<f32>,
    episodes_count: Option<u32>,
) {
    let mut meta_spans = vec![Span::styled(
        format!("[{}]", cat_badge.trim()),
        Style::default().fg(cat_color).add_modifier(Modifier::BOLD),
    )];
    if let Some(s) = score {
        meta_spans.push(Span::styled(
            format!("  ★ {:.1}", s),
            Style::default().fg(palette::HIGHLIGHT),
        ));
    }
    if let Some(eps) = episodes_count {
        meta_spans.push(Span::styled(
            format!("  · {} eps", eps),
            Style::default().fg(palette::TEXT_DIM),
        ));
    }

    let lines = vec![
        Line::from(Span::styled(
            title,
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(meta_spans),
    ];

    let widget = Paragraph::new(lines).wrap(Wrap { trim: true });
    f.render_widget(widget, area);
}

#[allow(clippy::too_many_arguments)]
fn render_compact_preview(
    f: &mut Frame,
    area: Rect,
    title: &str,
    cat_badge: &str,
    cat_color: Color,
    score: Option<f32>,
    episodes_count: Option<u32>,
    synopsis: &str,
) {
    let mut header = vec![
        Span::styled(
            title,
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  [{}]", cat_badge.trim()),
            Style::default().fg(cat_color),
        ),
    ];
    if let Some(s) = score {
        header.push(Span::styled(
            format!("  ★ {:.1}", s),
            Style::default().fg(palette::HIGHLIGHT),
        ));
    }
    if let Some(eps) = episodes_count {
        header.push(Span::styled(
            format!("  · {} eps", eps),
            Style::default().fg(palette::TEXT_DIM),
        ));
    }

    let syn_text = if synopsis.is_empty() {
        "Sin sinopsis disponible."
    } else {
        synopsis
    };

    let p = Paragraph::new(vec![
        Line::from(header),
        Line::from(Span::styled(syn_text, Style::default().fg(palette::TEXT_DIM))),
    ])
    .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::Episode;
    use image::{DynamicImage, ImageBuffer, Rgb};
    use ratatui::{Terminal, backend::TestBackend};
    use ratatui_image::picker::ProtocolType;

    fn sample_anime(cover_url: Option<&str>) -> Anime {
        Anime {
            id: "190".into(),
            title: "Naruto".into(),
            slug: "naruto".into(),
            synopsis: "Historia ninja en Konoha.".into(),
            category: AnimeCategory::TvAnime,
            mal_id: Some(20),
            episodes_count: Some(220),
            score: Some(8.2),
            cover_url: cover_url.map(str::to_string),
        }
    }

    #[test]
    fn results_and_episodes_render_covers_across_terminal_sizes() {
        let cover = "https://cdn.animeav1.com/covers/190.jpg";
        let (resize_tx, mut resize_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut picker = crate::cover::default_picker();
        picker.set_protocol_type(ProtocolType::Kitty);

        let mut app = App::new();
        app.init_cover_worker(picker, resize_tx);
        app.set_search_results(vec![sample_anime(Some(cover))], 1, 1, "naruto".into());

        let img = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(300, 425, Rgb([240, 140, 40])));
        app.handle_cover_loaded(cover.into(), Some(img));

        // 1. Render ancho en Results -> dispara ResizeRequest y muestra título + sinopsis
        let mut wide = Terminal::new(TestBackend::new(110, 32)).unwrap();
        wide.draw(|f| render(f, &mut app)).unwrap();
        let req = resize_rx.try_recv().expect("debió encolar redimensionado");
        let completed = req.resize_encode().expect("codificación Kitty OK");
        app.handle_cover_resized(completed);
        assert!(app.cover_frame_ready);
        wide.draw(|f| render(f, &mut app)).unwrap();

        let buf_text: String = wide
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf_text.contains("Naruto"));
        assert!(buf_text.contains("Historia ninja"));

        // 2. Transición a Episodes (misma portada, tamaño ligeramente distinto)
        app.set_episodes(
            sample_anime(Some(cover)),
            vec![Episode {
                id: 1,
                number: 1,
                anime_slug: "naruto".into(),
            }],
        );
        wide.draw(|f| render(f, &mut app)).unwrap();
        if let Ok(req2) = resize_rx.try_recv() {
            app.handle_cover_resized(req2.resize_encode().unwrap());
        }
        wide.draw(|f| render(f, &mut app)).unwrap();

        // 3. Terminales apiladas y mínimas sin panic
        let mut stacked = Terminal::new(TestBackend::new(60, 26)).unwrap();
        stacked.draw(|f| render(f, &mut app)).unwrap();

        let mut tiny = Terminal::new(TestBackend::new(30, 12)).unwrap();
        tiny.draw(|f| render(f, &mut app)).unwrap();
    }
}

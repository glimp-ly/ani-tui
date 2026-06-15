// ============================================================
// src/main.rs — Punto de entrada de ani-tui
// ============================================================
//
// Inicializa la terminal, lanza el event loop de la TUI y
// restaura el estado del terminal al salir.
//
// Subcomandos disponibles:
//   ani-tui          → Lanza la TUI interactiva (modo por defecto)
//   ani-tui search   → Búsqueda simple sin TUI (salida texto)
//   ani-tui serve    → API REST en modo servidor (puerto 3030)
// ============================================================

mod app;
mod config;
mod logger;
mod player;
mod resolver;
mod scraper;
mod security;
mod structs;
mod ui;

#[cfg(feature = "serve")]
mod routes;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::event::{KeyCode, KeyModifiers};
use tokio::sync::mpsc::UnboundedSender;

use app::{App, Screen};
use structs::AudioType;

// ————————————————————————————————————————————————
// CLI con Clap
// ————————————————————————————————————————————————

/// ani-tui — Busca y reproduce anime desde la terminal, sin anuncios
#[derive(Parser, Debug)]
#[command(
    name = "ani-tui",
    version = "0.2.0",
    author = "glimp",
    about = "🎌 TUI para buscar y reproducir anime sin anuncios",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Lanza la interfaz TUI interactiva (modo por defecto)
    Tui,

    /// Busca anime sin abrir la TUI (salida en texto plano)
    Search {
        /// Nombre del anime a buscar
        query: String,

        /// Página de resultados (por defecto: 1)
        #[arg(short, long, default_value_t = 1)]
        page: u32,
    },

    /// Levanta la API REST en el puerto 3030 (modo legacy)
    #[cfg(feature = "serve")]
    Serve {
        /// Puerto del servidor (por defecto: 3030)
        #[arg(short, long, default_value_t = config::SERVE_PORT)]
        port: u16,
    },
}

// ————————————————————————————————————————————————
// Entry point
// ————————————————————————————————————————————————

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // Sin subcomando → TUI interactiva
        None | Some(Commands::Tui) => {
            run_tui().await?;
        }

        // Subcomando search → búsqueda en texto plano
        Some(Commands::Search { query, page }) => {
            run_search_plain(&query, page).await?;
        }

        // Subcomando serve → API REST
        #[cfg(feature = "serve")]
        Some(Commands::Serve { port }) => {
            run_serve(port).await?;
        }
    }

    Ok(())
}

// ————————————————————————————————————————————————
// Modo TUI
// ————————————————————————————————————————————————

/// Lanza la TUI interactiva con Ratatui + Crossterm.
async fn run_tui() -> Result<()> {
    use crossterm::{
        execute,
        terminal::{
            disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
        },
        event::{self, Event, KeyEventKind},
    };
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::io;
    use std::time::Duration;
    use tokio::sync::mpsc;

    // — Inicializar logger (primero de todo) —
    // El logger registra todos los eventos internos a ~/.local/share/ani-tui/logs/
    if let Some(log_path) = logger::get_log_path() {
        eprintln!("[ani-tui] Logs en: {}", log_path.display());
    }

    // — Configurar terminal —
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // — Inicializar estado de la aplicación —
    let mut app = App::new();

    // — Canal para resultados asíncronos —
    // Los scrapers corren en tareas Tokio separadas y envían resultados
    // de vuelta al event loop mediante este canal.
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    // — Pantalla anterior para el overlay de ayuda —
    let mut prev_screen = Screen::Search;

    // ————————————————————————————————
    // Event loop principal
    // ————————————————————————————————
    loop {
        // — Renderizar UI —
        terminal.draw(|f| ui::render(f, &app))?;

        // — Procesar mensajes de tareas asíncronas (no bloqueante) —
        while let Ok(event) = rx.try_recv() {
            handle_app_event(&mut app, event);
        }

        // — Procesar evento de input (timeout 100ms para animar spinner) —
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Limpiar error al presionar cualquier tecla
                if app.error_message.is_some() {
                    app.error_message = None;
                    continue;
                }

                // No procesar input mientras carga
                if app.loading.is_loading() {
                    continue;
                }

                // — Manejar input según pantalla activa —
                match app.screen {
                    Screen::Search => handle_search_input(
                        &mut app, key.code, key.modifiers, &tx
                    ).await,

                    Screen::Results => handle_results_input(
                        &mut app, key.code, key.modifiers, &tx
                    ).await,

                    Screen::Episodes => handle_episodes_input(
                        &mut app, key.code, &tx
                    ).await,

                    Screen::Sources => handle_sources_input(
                        &mut app, key.code
                    ),

                    Screen::Help => {
                        // Cualquier tecla cierra el modal de ayuda
                        app.screen = prev_screen.clone();
                    }
                }

                // Abrir ayuda con '?'
                if key.code == KeyCode::Char('?') && app.screen != Screen::Help {
                    prev_screen = app.screen.clone();
                    app.screen = Screen::Help;
                }
            }
        } else {
            // Timeout → avanzar animación del spinner si hay carga
            if app.loading.is_loading() {
                app.tick_spinner();
            }
        }

        if app.should_quit {
            break;
        }
    }

    // — Restaurar terminal —
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

// ————————————————————————————————————————————————
// Eventos de tareas asíncronas → App
// ————————————————————————————————————————————————

/// Eventos que las tareas asíncronas envían al event loop principal.
enum AppEvent {
    /// Resultados de búsqueda recibidos
    SearchResults {
        animes: Vec<structs::Anime>,
        page: u32,
        total_pages: u32,
        query: String,
    },
    /// Episodios de un anime cargados
    EpisodesLoaded {
        anime: structs::Anime,
        episodes: Vec<structs::Episode>,
    },
    /// Fuentes de video cargadas
    SourcesLoaded {
        episode: structs::Episode,
        sources: structs::EpisodeSources,
    },
    /// Error en una operación asíncrona
    Error(String),
}

/// Procesa un evento de tarea asíncrona y actualiza el estado de la app.
fn handle_app_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::SearchResults { animes, page, total_pages, query } => {
            if animes.is_empty() {
                app.set_error(format!("No se encontraron resultados para \"{}\"", query));
            } else {
                app.set_search_results(animes, page, total_pages, query);
            }
        }
        AppEvent::EpisodesLoaded { anime, episodes } => {
            if episodes.is_empty() {
                app.set_error("No se encontraron episodios para este anime".to_string());
            } else {
                app.set_episodes(anime, episodes);
            }
        }
        AppEvent::SourcesLoaded { episode, sources } => {
            app.set_sources(episode, sources);
        }
        AppEvent::Error(msg) => {
            app.set_error(msg);
        }
    }
}

// ————————————————————————————————————————————————
// Manejadores de input por pantalla
// ————————————————————————————————————————————————

/// Maneja el input de teclado en la pantalla de búsqueda.
async fn handle_search_input(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    tx: &UnboundedSender<AppEvent>,
) {
    match code {
        // Salir
        KeyCode::Esc | KeyCode::Char('q') if app.search_input.is_empty() => {
            app.should_quit = true;
        }
        // Borrar campo completo con Ctrl+U
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.search_input.clear();
            app.cursor_position = 0;
        }
        // Borrar carácter
        KeyCode::Backspace => app.delete_char(),
        // Inicio/fin del campo
        KeyCode::Home => app.cursor_home(),
        KeyCode::End => app.cursor_end(),
        // Buscar
        KeyCode::Enter => {
            let query = app.search_input.trim().to_string();
            if !query.is_empty() {
                trigger_search(app, tx, query, 1).await;
            }
        }
        // Input de texto
        KeyCode::Char(c) => app.input_char(c),
        _ => {}
    }
}

/// Inicia una búsqueda asíncrona y envía resultados al channel.
async fn trigger_search(
    app: &mut App,
    tx: &UnboundedSender<AppEvent>,
    query: String,
    page: u32,
) {
    app.loading = app::LoadingState::SearchingAnime;
    let tx = tx.clone();
    let q = query.clone();

    tokio::spawn(async move {
        match scraper::search_anime(&q, page).await {
            Ok(result) => {
                let _ = tx.send(AppEvent::SearchResults {
                    animes: result.animes,
                    page: result.pagination.current_page,
                    total_pages: result.pagination.total_pages,
                    query: q,
                });
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(format!("Error al buscar: {}", e)));
            }
        }
    });
}

/// Maneja el input de teclado en la pantalla de resultados.
async fn handle_results_input(
    app: &mut App,
    code: KeyCode,
    _modifiers: KeyModifiers,
    tx: &UnboundedSender<AppEvent>,
) {
    match code {
        KeyCode::Esc => app.go_back(),
        KeyCode::Up | KeyCode::Char('k') => app.list_up(),
        KeyCode::Down | KeyCode::Char('j') => app.list_down(),
        // Ir a pantalla de búsqueda con '/'
        KeyCode::Char('/') => {
            app.screen = app::Screen::Search;
        }
        // Página siguiente
        KeyCode::Right if app.results_page < app.results_total_pages => {
            if let Some(ref query) = app.last_query.clone() {
                trigger_search(app, tx, query.clone(), app.results_page + 1).await;
            }
        }
        // Página anterior
        KeyCode::Left if app.results_page > 1 => {
            if let Some(ref query) = app.last_query.clone() {
                trigger_search(app, tx, query.clone(), app.results_page - 1).await;
            }
        }
        // Seleccionar anime y cargar episodios
        KeyCode::Enter => {
            if let Some(idx) = app.results_state.selected() {
                if let Some(anime) = app.search_results.get(idx).cloned() {
                    trigger_load_episodes(app, tx, anime).await;
                }
            }
        }
        _ => {}
    }
}

/// Inicia la carga asíncrona de episodios para un anime.
async fn trigger_load_episodes(
    app: &mut App,
    tx: &UnboundedSender<AppEvent>,
    anime: structs::Anime,
) {
    app.loading = app::LoadingState::LoadingEpisodes;
    let tx = tx.clone();

    tokio::spawn(async move {
        let mut a = anime.clone();
        match scraper::get_episodes(&mut a).await {
            Ok(episodes) => {
                let _ = tx.send(AppEvent::EpisodesLoaded { anime: a, episodes });
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(format!("Error al cargar episodios: {}", e)));
            }
        }
    });
}

/// Maneja el input de teclado en la pantalla de episodios.
async fn handle_episodes_input(
    app: &mut App,
    code: KeyCode,
    tx: &UnboundedSender<AppEvent>,
) {
    let page_size = 10usize;

    match code {
        KeyCode::Esc => app.go_back(),
        KeyCode::Up | KeyCode::Char('k') => app.list_up(),
        KeyCode::Down | KeyCode::Char('j') => app.list_down(),
        // Saltar página arriba/abajo
        KeyCode::PageUp => {
            for _ in 0..page_size { app.list_up(); }
        }
        KeyCode::PageDown => {
            for _ in 0..page_size { app.list_down(); }
        }
        // Ir al inicio/fin
        KeyCode::Home => {
            if !app.episodes.is_empty() {
                app.episodes_state.select(Some(0));
            }
        }
        KeyCode::End => {
            let len = app.episodes.len();
            if len > 0 {
                app.episodes_state.select(Some(len - 1));
            }
        }
        // Cargar fuentes del episodio seleccionado
        KeyCode::Enter => {
            if let Some(idx) = app.episodes_state.selected() {
                if let Some(episode) = app.episodes.get(idx).cloned() {
                    trigger_load_sources(app, tx, episode).await;
                }
            }
        }
        _ => {}
    }
}

/// Inicia la carga asíncrona de fuentes de video para un episodio.
async fn trigger_load_sources(
    app: &mut App,
    tx: &UnboundedSender<AppEvent>,
    episode: structs::Episode,
) {
    app.loading = app::LoadingState::LoadingSources;
    let tx = tx.clone();

    tokio::spawn(async move {
        let ep = episode.clone();
        match scraper::get_video_sources(&ep.anime_slug, ep.number).await {
            Ok(sources) => {
                let _ = tx.send(AppEvent::SourcesLoaded { episode: ep, sources });
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(format!("Error al cargar fuentes: {}", e)));
            }
        }
    });
}

/// Maneja el input de teclado en la pantalla de fuentes de video.
fn handle_sources_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.go_back(),
        KeyCode::Up | KeyCode::Char('k') => app.list_up(),
        KeyCode::Down | KeyCode::Char('j') => app.list_down(),
        // Cambiar entre SUB y DUB
        KeyCode::Tab | KeyCode::BackTab => {
            if app.sources.has_dub() {
                app.toggle_audio();
            }
        }
        KeyCode::Char('s') => {
            if app.selected_audio != AudioType::Sub {
                app.toggle_audio();
            }
        }
        KeyCode::Char('d') => {
            if app.sources.has_dub() && app.selected_audio != AudioType::Dub {
                app.toggle_audio();
            }
        }
        // Reproducir la fuente seleccionada (async: resolver URL + mpv/browser)
        KeyCode::Enter => {
            let sources = app.sources.get(&app.selected_audio).to_vec();
            if let Some(idx) = app.sources_state.selected() {
                if let Some(source) = sources.get(idx).cloned() {
                    // Usar la versión que resuelve la URL primero
                    let result = player::play_source(&source);
                    match result {
                        player::PlayResult::LaunchedMpv { ref url } => {
                            logger::log_info("tui", &format!("mpv lanzado: {}", &url[..url.len().min(50)]));
                        }
                        player::PlayResult::LaunchedBrowser { .. } => {
                            logger::log_info("tui", "Navegador abierto");
                        }
                        player::PlayResult::NoSources => {
                            app.set_error("No hay fuentes disponibles".to_string());
                        }
                        player::PlayResult::Error(ref e) => {
                            app.set_error(format!("Error al reproducir: {}", e));
                        }
                    }
                    // Mostrar mensaje de estado en la TUI
                    let msg = result.display_message();
                    // El mensaje se muestra brevemente como info (no error)
                    if matches!(result, player::PlayResult::Error(_) | player::PlayResult::NoSources) {
                        // error ya establecido arriba
                    } else {
                        app.error_message = Some(msg); // reutilizar para mensaje de info
                    }
                }
            }
        }
        _ => {}
    }
}

// ————————————————————————————————————————————————
// Modo búsqueda simple (sin TUI)
// ————————————————————————————————————————————————

/// Ejecuta una búsqueda y muestra los resultados en texto plano.
/// Útil para scripting o uso en pipelines de shell.
async fn run_search_plain(query: &str, page: u32) -> Result<()> {
    println!("🔍 Buscando \"{}\" (página {})...", query, page);

    match scraper::search_anime(query, page).await {
        Ok(result) => {
            println!(
                "📋 {} resultados (página {}/{}):\n",
                result.pagination.total_records,
                result.pagination.current_page,
                result.pagination.total_pages
            );
            for (i, anime) in result.animes.iter().enumerate() {
                println!(
                    "  {:2}. [{}] {}",
                    i + 1,
                    anime.category.display(),
                    anime.title
                );
                println!("      URL: {}", anime.page_url(config::BASE_URL));
                if !anime.synopsis.is_empty() {
                    // Mostrar primer párrafo de la sinopsis
                    let first_line = anime.synopsis.lines().next().unwrap_or("");
                    let truncated = if first_line.len() > 80 {
                        format!("{}...", &first_line[..80])
                    } else {
                        first_line.to_string()
                    };
                    println!("      {}", truncated);
                }
                println!();
            }
        }
        Err(e) => {
            eprintln!("❌ Error al buscar: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

// ————————————————————————————————————————————————
// Modo API REST (subcomando serve)
// ————————————————————————————————————————————————

#[cfg(feature = "serve")]
async fn run_serve(port: u16) -> Result<()> {
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tower_http::cors::{CorsLayer, Any};

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = routes::create_routes().layer(cors);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;
    println!("🚀 API REST iniciada en http://{}", addr);
    println!("   Endpoints:");
    println!("   GET /search?q=<query>              — buscar anime");
    println!("   GET /anime/<slug>/episodes          — listar episodios");
    println!("   GET /episode/<slug>/<number>/sources — fuentes de video");

    axum::serve(listener, app).await?;
    Ok(())
}
// ============================================================
// src/app.rs — Estado central de la aplicación TUI
// ============================================================
//
// Define el estado global de la aplicación (AppState) y el enum
// de pantallas (Screen). Toda la lógica de navegación entre
// pantallas y actualización de estado pasa por este módulo.
//
// La TUI usa un modelo de estado centralizado (similar a Elm/Redux):
// - Un único `App` contiene todo el estado mutable
// - Las funciones de render leen el estado (inmutable)
// - Los eventos de input modifican el estado
// ============================================================

use ratatui::widgets::ListState;

use crate::structs::{Anime, AudioType, Episode, EpisodeSources};
use crate::player::PlayerInfo;

// ————————————————————————————————————————————————
// Pantallas disponibles en la TUI
// ————————————————————————————————————————————————

/// Pantalla activa en la TUI.
/// La navegación sigue el flujo: Search → Results → Episodes → Sources
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// Pantalla de búsqueda — input de texto para buscar anime
    Search,
    /// Lista de resultados de búsqueda
    Results,
    /// Lista de episodios del anime seleccionado
    Episodes,
    /// Selección de servidor/fuente de video para el episodio seleccionado
    Sources,
    /// Modal de ayuda con keybindings (overlay sobre cualquier pantalla)
    Help,
}

// ————————————————————————————————————————————————
// Estado de operaciones asíncronas
// ————————————————————————————————————————————————

/// Estado de una operación asíncrona en curso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadingState {
    /// No hay operación en curso
    Idle,
    /// Buscando anime...
    SearchingAnime,
    /// Cargando lista de episodios...
    LoadingEpisodes,
    /// Cargando fuentes de video...
    LoadingSources,
}

impl LoadingState {
    /// Mensaje de carga para mostrar en la TUI.
    pub fn message(&self) -> Option<&str> {
        match self {
            LoadingState::Idle => None,
            LoadingState::SearchingAnime => Some("Buscando anime..."),
            LoadingState::LoadingEpisodes => Some("Cargando episodios..."),
            LoadingState::LoadingSources => Some("Cargando fuentes de video..."),
        }
    }

    /// Indica si hay una operación en curso.
    pub fn is_loading(&self) -> bool {
        *self != LoadingState::Idle
    }
}

// ————————————————————————————————————————————————
// Estado de la aplicación
// ————————————————————————————————————————————————

/// Estado central de la aplicación TUI.
///
/// Contiene todo el estado mutable de la aplicación.
/// Las funciones de render reciben `&App` (solo lectura).
/// Los manejadores de eventos reciben `&mut App` (escritura).
pub struct App {
    // — Navegación —
    /// Pantalla actualmente visible
    pub screen: Screen,

    /// Si es true, la aplicación debe terminar en el próximo ciclo
    pub should_quit: bool,

    // — Búsqueda —
    /// Texto ingresado por el usuario en el campo de búsqueda
    pub search_input: String,

    /// Posición del cursor en el campo de búsqueda
    pub cursor_position: usize,

    /// Si hay una búsqueda completada (texto de la última búsqueda exitosa)
    pub last_query: Option<String>,

    // — Resultados de búsqueda —
    /// Lista de animes encontrados en la búsqueda actual
    pub search_results: Vec<Anime>,

    /// Estado de la lista de resultados (item seleccionado, scroll)
    pub results_state: ListState,

    /// Página actual de resultados (base 1)
    pub results_page: u32,

    /// Total de páginas disponibles para la búsqueda actual
    pub results_total_pages: u32,

    // — Episodios —
    /// Anime seleccionado por el usuario
    pub selected_anime: Option<Anime>,

    /// Lista de episodios del anime seleccionado
    pub episodes: Vec<Episode>,

    /// Estado de la lista de episodios (item seleccionado, scroll)
    pub episodes_state: ListState,

    // — Fuentes de video —
    /// Episodio seleccionado por el usuario
    pub selected_episode: Option<Episode>,

    /// Fuentes de video disponibles (SUB y DUB)
    pub sources: EpisodeSources,

    /// Tipo de audio actualmente seleccionado (SUB/DUB)
    pub selected_audio: AudioType,

    /// Estado de la lista de servidores de video
    pub sources_state: ListState,

    // — Estado de UI —
    /// Estado de carga de operaciones asíncronas
    pub loading: LoadingState,

    /// Mensaje de error para mostrar al usuario (None = sin error)
    pub error_message: Option<String>,

    /// Mensaje informativo para mostrar al usuario (None = sin mensaje)
    pub info_message: Option<String>,

    /// Información sobre reproductores disponibles en el sistema
    pub player_info: PlayerInfo,

    /// Contador de animación del spinner de carga (0-7)
    pub spinner_frame: u8,
}

impl App {
    /// Crea una nueva instancia de la aplicación con estado inicial.
    pub fn new() -> Self {
        App {
            screen: Screen::Search,
            should_quit: false,

            search_input: String::new(),
            cursor_position: 0,
            last_query: None,

            search_results: vec![],
            results_state: ListState::default(),
            results_page: 1,
            results_total_pages: 1,

            selected_anime: None,
            episodes: vec![],
            episodes_state: ListState::default(),

            selected_episode: None,
            sources: EpisodeSources::default(),
            selected_audio: AudioType::Sub,
            sources_state: ListState::default(),

            loading: LoadingState::Idle,
            error_message: None,
            info_message: None,
            player_info: crate::player::player_info(),
            spinner_frame: 0,
        }
    }

    // ————————————————————————————————————————
    // Navegación
    // ————————————————————————————————————————

    /// Vuelve a la pantalla anterior en la jerarquía de navegación.
    /// - Sources → Episodes
    /// - Episodes → Results
    /// - Results → Search
    /// - Search → quit (si el campo está vacío)
    pub fn go_back(&mut self) {
        self.error_message = None;
        self.info_message = None;
        match self.screen {
            Screen::Sources => self.screen = Screen::Episodes,
            Screen::Episodes => self.screen = Screen::Results,
            Screen::Results => {
                self.screen = Screen::Search;
                self.search_results.clear();
                self.results_state = ListState::default();
            }
            Screen::Search => {
                if self.search_input.is_empty() {
                    self.should_quit = true;
                } else {
                    self.search_input.clear();
                    self.cursor_position = 0;
                }
            }
            Screen::Help => {
                // El Help es un overlay — volver a la pantalla que estaba antes
                // (se maneja en el manejador de eventos)
                self.screen = Screen::Search;
            }
        }
    }

    // ————————————————————————————————————————
    // Input de texto
    // ————————————————————————————————————————

    /// Agrega un carácter al campo de búsqueda en la posición del cursor.
    pub fn input_char(&mut self, c: char) {
        self.search_input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    /// Elimina el carácter anterior al cursor (Backspace).
    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.search_input.remove(self.cursor_position);
        }
    }

    /// Mueve el cursor al inicio del campo de búsqueda.
    pub fn cursor_home(&mut self) {
        self.cursor_position = 0;
    }

    /// Mueve el cursor al final del campo de búsqueda.
    pub fn cursor_end(&mut self) {
        self.cursor_position = self.search_input.len();
    }

    // ————————————————————————————————————————
    // Navegación en listas
    // ————————————————————————————————————————

    /// Mueve la selección hacia arriba en la lista activa.
    pub fn list_up(&mut self) {
        match self.screen {
            Screen::Results => {
                let i = self.results_state.selected().unwrap_or(0);
                let new = if i == 0 {
                    self.search_results.len().saturating_sub(1)
                } else {
                    i - 1
                };
                self.results_state.select(Some(new));
            }
            Screen::Episodes => {
                let i = self.episodes_state.selected().unwrap_or(0);
                let new = if i == 0 {
                    self.episodes.len().saturating_sub(1)
                } else {
                    i - 1
                };
                self.episodes_state.select(Some(new));
            }
            Screen::Sources => {
                let sources = self.sources.get(&self.selected_audio);
                let i = self.sources_state.selected().unwrap_or(0);
                let new = if i == 0 {
                    sources.len().saturating_sub(1)
                } else {
                    i - 1
                };
                self.sources_state.select(Some(new));
            }
            _ => {}
        }
    }

    /// Mueve la selección hacia abajo en la lista activa.
    pub fn list_down(&mut self) {
        match self.screen {
            Screen::Results => {
                let len = self.search_results.len();
                if len == 0 { return; }
                let i = self.results_state.selected().unwrap_or(0);
                self.results_state.select(Some((i + 1) % len));
            }
            Screen::Episodes => {
                let len = self.episodes.len();
                if len == 0 { return; }
                let i = self.episodes_state.selected().unwrap_or(0);
                self.episodes_state.select(Some((i + 1) % len));
            }
            Screen::Sources => {
                let len = self.sources.get(&self.selected_audio).len();
                if len == 0 { return; }
                let i = self.sources_state.selected().unwrap_or(0);
                self.sources_state.select(Some((i + 1) % len));
            }
            _ => {}
        }
    }

    // ————————————————————————————————————————
    // Actualización de estado tras operaciones asíncronas
    // ————————————————————————————————————————

    /// Actualiza los resultados de búsqueda con los datos recibidos.
    pub fn set_search_results(
        &mut self,
        animes: Vec<Anime>,
        page: u32,
        total_pages: u32,
        query: String,
    ) {
        self.search_results = animes;
        self.results_page = page;
        self.results_total_pages = total_pages;
        self.last_query = Some(query);
        self.loading = LoadingState::Idle;
        self.error_message = None;
        self.info_message = None;

        // Seleccionar el primer resultado automáticamente
        if !self.search_results.is_empty() {
            self.results_state.select(Some(0));
            self.screen = Screen::Results;
        }
    }

    /// Actualiza la lista de episodios del anime seleccionado.
    pub fn set_episodes(&mut self, anime: Anime, episodes: Vec<Episode>) {
        self.selected_anime = Some(anime);
        self.episodes = episodes;
        self.loading = LoadingState::Idle;
        self.error_message = None;
        self.info_message = None;

        if !self.episodes.is_empty() {
            self.episodes_state.select(Some(0));
            self.screen = Screen::Episodes;
        }
    }

    /// Actualiza las fuentes de video del episodio seleccionado.
    pub fn set_sources(&mut self, episode: Episode, sources: EpisodeSources) {
        self.selected_episode = Some(episode);
        self.sources = sources;
        self.loading = LoadingState::Idle;
        self.error_message = None;
        self.info_message = None;
        self.selected_audio = AudioType::Sub;

        // Seleccionar primera fuente automáticamente
        if !self.sources.sub.is_empty() {
            self.sources_state.select(Some(0));
        }
        self.screen = Screen::Sources;
    }

    /// Establece un mensaje de error para mostrar al usuario.
    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.error_message = Some(msg.into());
        self.loading = LoadingState::Idle;
    }

    /// Establece un mensaje informativo para mostrar al usuario.
    pub fn set_info(&mut self, msg: impl Into<String>) {
        self.info_message = Some(msg.into());
    }

    /// Alterna entre audio SUB y DUB en la pantalla de fuentes.
    pub fn toggle_audio(&mut self) {
        self.selected_audio = match self.selected_audio {
            AudioType::Sub => AudioType::Dub,
            AudioType::Dub => AudioType::Sub,
        };
        // Resetear selección al cambiar de audio
        let len = self.sources.get(&self.selected_audio).len();
        if len > 0 {
            self.sources_state.select(Some(0));
        } else {
            self.sources_state.select(None);
        }
    }

    /// Avanza el frame del spinner de animación.
    pub fn tick_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % 8;
    }

    /// Devuelve el carácter actual del spinner de carga.
    pub fn spinner_char(&self) -> char {
        const FRAMES: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];
        FRAMES[self.spinner_frame as usize]
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

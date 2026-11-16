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

use std::time::{Duration, Instant};

use image::DynamicImage;
use ratatui::widgets::ListState;
use ratatui_image::{
    picker::{Picker, ProtocolType},
    thread::{ResizeRequest, ResizeResponse, ThreadProtocol},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::cover::{self, CoverCache, CoverEntry};
use crate::player::PlayerInfo;
use crate::structs::{Anime, AudioType, Episode, EpisodeSources};

/// Estado visual de la portada activa para decidir qué dibujar en el panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverDisplayState {
    Unavailable,
    Loading,
    Ready { frame_encoded: bool },
}

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
    /// Resolviendo e iniciando reproductor...
    LaunchingPlayer,
}

impl LoadingState {
    /// Mensaje de carga para mostrar en la TUI.
    pub fn message(&self) -> Option<&str> {
        match self {
            LoadingState::Idle => None,
            LoadingState::SearchingAnime => Some("Buscando anime..."),
            LoadingState::LoadingEpisodes => Some("Cargando episodios..."),
            LoadingState::LoadingSources => Some("Cargando fuentes de video..."),
            LoadingState::LaunchingPlayer => Some("Iniciando reproductor..."),
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

    // — Portadas de anime —
    /// Picker de protocolo gráfico soportado por la terminal
    pub cover_picker: Picker,

    /// Estado del widget de imagen con redimensionado en segundo plano
    pub cover_protocol: ThreadProtocol,

    /// Caché en memoria de portadas decodificadas
    pub cover_cache: CoverCache,

    /// URL de la portada actualmente seleccionada
    pub active_cover_url: Option<String>,

    /// Indica si la portada activa ya tiene al menos un frame codificado
    pub cover_frame_ready: bool,

    /// URL pendiente de descarga con debounce para evitar ráfagas al navegar rápido
    pending_cover_fetch: Option<(String, Instant)>,
}

impl App {
    /// Crea una nueva instancia de la aplicación con estado inicial.
    pub fn new() -> Self {
        let (noop_resize_tx, _noop_resize_rx) = tokio::sync::mpsc::unbounded_channel();

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

            cover_picker: cover::default_picker(),
            cover_protocol: ThreadProtocol::new(noop_resize_tx, None),
            cover_cache: CoverCache::default(),
            active_cover_url: None,
            cover_frame_ready: false,
            pending_cover_fetch: None,
        }
    }

    /// Configura el picker detectado y el canal real del worker de redimensionado.
    pub fn init_cover_worker(
        &mut self,
        picker: Picker,
        resize_tx: UnboundedSender<ResizeRequest>,
    ) {
        self.cover_picker = picker;
        self.cover_protocol = ThreadProtocol::new(resize_tx, None);
        self.cover_frame_ready = false;
        self.sync_cover_selection(true);
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
            Screen::Sources => {
                self.screen = Screen::Episodes;
                self.sync_cover_selection(true);
            }
            Screen::Episodes => {
                self.screen = Screen::Results;
                self.sync_cover_selection(true);
            }
            Screen::Results => {
                self.screen = Screen::Search;
                self.search_results.clear();
                self.results_state = ListState::default();
                self.sync_cover_selection(true);
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
        let pos = self.cursor_position.min(self.search_input.len());
        self.search_input.insert(pos, c);
        self.cursor_position = pos + c.len_utf8();
    }

    /// Elimina el carácter anterior al cursor (Backspace).
    pub fn delete_char(&mut self) {
        let pos = self.cursor_position.min(self.search_input.len());
        if pos > 0 {
            if let Some((prev_idx, _)) = self.search_input[..pos].char_indices().next_back() {
                self.search_input.remove(prev_idx);
                self.cursor_position = prev_idx;
            }
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
                let len = self.search_results.len();
                if len == 0 {
                    return;
                }
                let i = self.results_state.selected().unwrap_or(0);
                let new = if i == 0 { len - 1 } else { i - 1 };
                self.results_state.select(Some(new));
                self.sync_cover_selection(false);
            }
            Screen::Episodes => {
                let len = self.episodes.len();
                if len == 0 {
                    return;
                }
                let i = self.episodes_state.selected().unwrap_or(0);
                let new = if i == 0 { len - 1 } else { i - 1 };
                self.episodes_state.select(Some(new));
            }
            Screen::Sources => {
                let len = self.sources.get(&self.selected_audio).len();
                if len == 0 {
                    return;
                }
                let i = self.sources_state.selected().unwrap_or(0);
                let new = if i == 0 { len - 1 } else { i - 1 };
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
                self.sync_cover_selection(false);
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
            self.sync_cover_selection(true);
        }
    }

    /// Actualiza la lista de episodios del anime seleccionado.
    pub fn set_episodes(&mut self, anime: Anime, episodes: Vec<Episode>) {
        if let Some(existing) = self.search_results.iter_mut().find(|a| a.slug == anime.slug) {
            *existing = anime.clone();
        }
        self.selected_anime = Some(anime);
        self.episodes = episodes;
        self.loading = LoadingState::Idle;
        self.error_message = None;
        self.info_message = None;

        if !self.episodes.is_empty() {
            self.episodes_state.select(Some(0));
            self.screen = Screen::Episodes;
            self.sync_cover_selection(true);
        }
    }

    /// Actualiza las fuentes de video del episodio seleccionado.
    pub fn set_sources(&mut self, episode: Episode, sources: EpisodeSources) {
        self.selected_episode = Some(episode);
        self.sources = sources;
        self.loading = LoadingState::Idle;
        self.error_message = None;
        self.info_message = None;
        // Seleccionar DUB automáticamente si no hay fuentes SUB disponibles
        self.selected_audio = if self.sources.sub.is_empty() && !self.sources.dub.is_empty() {
            AudioType::Dub
        } else {
            AudioType::Sub
        };

        self.select_default_source();
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
        self.select_default_source();
    }

    /// Prioriza automáticamente la mejor fuente compatible con mpv para el audio actual.
    fn select_default_source(&mut self) {
        let list = self.sources.get(&self.selected_audio);
        if list.is_empty() {
            self.sources_state.select(None);
            return;
        }
        let best_idx = self
            .sources
            .best_for_mpv(&self.selected_audio)
            .and_then(|best| {
                list.iter()
                    .position(|s| s.server == best.server && s.url == best.url)
            })
            .unwrap_or(0);
        self.sources_state.select(Some(best_idx));
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

    // ————————————————————————————————————————
    // Sincronización y caché de portadas
    // ————————————————————————————————————————

    /// Sincroniza la portada mostrada con el anime actualmente activo en pantalla.
    pub fn sync_cover_selection(&mut self, immediate: bool) {
        let desired = match self.screen {
            Screen::Results => self
                .results_state
                .selected()
                .and_then(|i| self.search_results.get(i))
                .and_then(|a| a.cover_url.clone()),
            Screen::Episodes => self
                .selected_anime
                .as_ref()
                .and_then(|a| a.cover_url.clone()),
            _ => None,
        };

        if desired == self.active_cover_url {
            return;
        }

        self.active_cover_url = desired.clone();
        self.pending_cover_fetch = None;

        let Some(url) = desired else {
            self.cover_protocol.empty_protocol();
            self.cover_frame_ready = false;
            return;
        };

        match self.cover_cache.get(&url) {
            Some(CoverEntry::Ready(img)) => {
                self.cover_protocol
                    .replace_protocol(self.cover_picker.new_resize_protocol(img.clone()));
                self.cover_frame_ready = false;
            }
            Some(CoverEntry::Loading) | Some(CoverEntry::Failed) => {
                self.cover_protocol.empty_protocol();
                self.cover_frame_ready = false;
            }
            None => {
                self.cover_protocol.empty_protocol();
                self.cover_frame_ready = false;
                let delay = if immediate {
                    Duration::ZERO
                } else {
                    Duration::from_millis(60)
                };
                self.pending_cover_fetch = Some((url, Instant::now() + delay));
            }
        }
    }

    /// Devuelve y marca como `Loading` la siguiente portada cuyo debounce venció.
    pub fn take_due_cover_fetch(&mut self) -> Option<String> {
        let (_, due_at) = self.pending_cover_fetch.as_ref()?;
        if Instant::now() < *due_at {
            return None;
        }
        let (url, _) = self.pending_cover_fetch.take()?;
        if self.cover_cache.get(&url).is_some() {
            return None;
        }
        self.cover_cache.mark_loading(&url);
        Some(url)
    }

    /// Registra el resultado de descargar/decodificar una portada en background.
    pub fn handle_cover_loaded(&mut self, url: String, image: Option<DynamicImage>) {
        match image {
            Some(img) => {
                self.cover_cache.insert_ready(url.clone(), img.clone());
                if self.active_cover_url.as_deref() == Some(url.as_str()) {
                    self.cover_protocol
                        .replace_protocol(self.cover_picker.new_resize_protocol(img));
                    self.cover_frame_ready = false;
                }
            }
            None => {
                self.cover_cache.insert_failed(url);
            }
        }
    }

    /// Aplica el resultado de redimensionado/codificación asíncrono al protocolo activo.
    pub fn handle_cover_resized(&mut self, completed: ResizeResponse) {
        if self.cover_protocol.update_resized_protocol(completed) {
            self.cover_frame_ready = true;
        }
    }

    /// Indica si hay descarga o codificación de portada activa para acortar el poll del loop.
    pub fn has_pending_cover_work(&self) -> bool {
        self.pending_cover_fetch.is_some()
            || self.active_cover_url.as_deref().is_some_and(|u| {
                matches!(self.cover_cache.get(u), Some(CoverEntry::Loading))
                    || (!self.cover_frame_ready
                        && matches!(self.cover_cache.get(u), Some(CoverEntry::Ready(_))))
            })
    }

    /// Estado de renderizado para la URL de portada solicitada.
    pub fn cover_state(&self, url: Option<&str>) -> CoverDisplayState {
        let Some(url) = url else {
            return CoverDisplayState::Unavailable;
        };
        if self.active_cover_url.as_deref() != Some(url) {
            return match self.cover_cache.get(url) {
                Some(CoverEntry::Failed) => CoverDisplayState::Unavailable,
                _ => CoverDisplayState::Loading,
            };
        }
        match self.cover_cache.get(url) {
            Some(CoverEntry::Ready(_)) => CoverDisplayState::Ready {
                frame_encoded: self.cover_frame_ready,
            },
            Some(CoverEntry::Failed) => CoverDisplayState::Unavailable,
            Some(CoverEntry::Loading) | None => CoverDisplayState::Loading,
        }
    }

    /// En protocolos de píxeles directos (Sixel/iTerm2), evita dibujar sobre modales centrados.
    pub fn should_suppress_cover_for_overlay(&self) -> bool {
        let has_overlay =
            self.loading.is_loading() || self.error_message.is_some() || self.info_message.is_some();
        has_overlay
            && matches!(
                self.cover_picker.protocol_type(),
                ProtocolType::Sixel | ProtocolType::Iterm2
            )
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::{AnimeCategory, VideoSource};

    #[test]
    fn utf8_input_and_delete_do_not_panic() {
        let mut app = App::new();
        for c in ['ñ', 'á', 'ó', 'x'] {
            app.input_char(c);
        }
        assert_eq!(app.search_input, "ñáóx");
        app.delete_char();
        app.delete_char();
        assert_eq!(app.search_input, "ñá");
        app.cursor_home();
        app.input_char('¡');
        assert_eq!(app.search_input, "¡ñá");
        app.cursor_end();
        app.input_char('!');
        assert_eq!(app.search_input, "¡ñá!");
    }

    #[test]
    fn set_sources_picks_dub_if_sub_empty_and_prefers_mpv_source() {
        let mut app = App::new();
        let ep = Episode {
            id: 1,
            number: 1,
            anime_slug: "test".into(),
        };
        let sources = EpisodeSources {
            sub: vec![],
            dub: vec![
                VideoSource {
                    server: "UPNShare".into(),
                    url: "https://example.com/a".into(),
                    audio: AudioType::Dub,
                    quality: None,
                },
                VideoSource {
                    server: "HLS".into(),
                    url: "https://player.zilla-networks.com/play/0123456789abcdef0123456789abcdef".into(),
                    audio: AudioType::Dub,
                    quality: None,
                },
            ],
        };

        app.set_sources(ep, sources);
        assert_eq!(app.selected_audio, AudioType::Dub);
        assert_eq!(app.sources_state.selected(), Some(1));
    }

    #[test]
    fn cover_selection_schedules_fetch_and_updates_from_cache() {
        let mut app = App::new();
        let a1 = Anime {
            id: "1".into(),
            title: "Naruto".into(),
            slug: "naruto".into(),
            synopsis: "Ninja".into(),
            category: AnimeCategory::TvAnime,
            mal_id: None,
            episodes_count: None,
            score: None,
            cover_url: Some("https://cdn.animeav1.com/covers/190.jpg".into()),
        };
        let a2 = Anime {
            id: "2".into(),
            title: "Lain".into(),
            slug: "serial-experiments-lain".into(),
            synopsis: "Wired".into(),
            category: AnimeCategory::TvAnime,
            mal_id: None,
            episodes_count: None,
            score: None,
            cover_url: Some("https://cdn.animeav1.com/covers/1684.jpg".into()),
        };

        app.set_search_results(vec![a1, a2], 1, 1, "test".into());
        assert_eq!(
            app.active_cover_url.as_deref(),
            Some("https://cdn.animeav1.com/covers/190.jpg")
        );
        assert_eq!(
            app.take_due_cover_fetch().as_deref(),
            Some("https://cdn.animeav1.com/covers/190.jpg")
        );
        assert!(app.take_due_cover_fetch().is_none());

        app.list_down();
        assert_eq!(
            app.active_cover_url.as_deref(),
            Some("https://cdn.animeav1.com/covers/1684.jpg")
        );
        assert_eq!(
            app.cover_state(app.active_cover_url.as_deref()),
            CoverDisplayState::Loading
        );
    }
}

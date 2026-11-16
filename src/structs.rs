// ============================================================
// src/structs.rs — Estructuras de datos compartidas
// ============================================================
//
// Define los tipos de datos que fluyen entre el scraper y la TUI.
// Todas las estructuras implementan Clone para facilitar su uso
// en el estado de la aplicación.
// ============================================================

use serde::{Deserialize, Serialize};

// ————————————————————————————————————————————————
// Tipos de anime (categorías del sitio)
// ————————————————————————————————————————————————

/// Tipo/categoría del anime según animeav1.com
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnimeCategory {
    /// Serie de televisión regular (categoryId: 1)
    TvAnime,
    /// Película (categoryId: 2)
    Movie,
    /// Original Video Animation (categoryId: 3)
    Ova,
    /// Episodio especial (categoryId: 4)
    Special,
    /// Original Net Animation (categoryId: 5)
    Ona,
    /// Categoría no reconocida
    Unknown(String),
}

impl AnimeCategory {
    /// Convierte el categoryId numérico del sitio a esta enum.
    pub fn from_id(id: u32) -> Self {
        match id {
            1 => AnimeCategory::TvAnime,
            2 => AnimeCategory::Movie,
            3 => AnimeCategory::Ova,
            4 => AnimeCategory::Special,
            5 => AnimeCategory::Ona,
            _ => AnimeCategory::Unknown(id.to_string()),
        }
    }

    /// Convierte el nombre de categoría del sitio a esta enum.
    pub fn from_name(name: &str) -> Self {
        match name {
            "TV Anime" | "tv-anime" => AnimeCategory::TvAnime,
            "Movie" | "movie"       => AnimeCategory::Movie,
            "OVA" | "ova"           => AnimeCategory::Ova,
            "Special" | "special"   => AnimeCategory::Special,
            "ONA" | "ona"           => AnimeCategory::Ona,
            other                   => AnimeCategory::Unknown(other.to_string()),
        }
    }

    /// Representación legible para mostrar en la TUI.
    pub fn display(&self) -> &str {
        match self {
            AnimeCategory::TvAnime        => "TV",
            AnimeCategory::Movie          => "Movie",
            AnimeCategory::Ova            => "OVA",
            AnimeCategory::Special        => "Especial",
            AnimeCategory::Ona            => "ONA",
            AnimeCategory::Unknown(_)     => "?",
        }
    }
}

// ————————————————————————————————————————————————
// Anime (resultado de búsqueda)
// ————————————————————————————————————————————————

/// Resultado de búsqueda de anime.
/// Contiene la información suficiente para mostrar en la lista de resultados.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anime {
    /// ID interno del sitio (como string, ej: "197")
    pub id: String,

    /// Título del anime (ej: "One Piece")
    pub title: String,

    /// Slug URL del anime (ej: "one-piece").
    /// Se usa para construir la URL: /media/{slug}
    pub slug: String,

    /// Sinopsis del anime (puede ser larga)
    pub synopsis: String,

    /// Categoría del anime (TV, Movie, OVA, Special)
    pub category: AnimeCategory,

    /// ID en MyAnimeList.net (puede ser None si no está disponible)
    pub mal_id: Option<u32>,

    /// Total de episodios disponibles (None si no se ha cargado aún)
    pub episodes_count: Option<u32>,

    /// Puntuación del anime (0.0 - 10.0)
    pub score: Option<f32>,

    /// URL de la imagen de portada del anime (puede ser None si no está disponible)
    pub cover_url: Option<String>,
}

impl Anime {
    /// Construye la URL completa de la página del anime.
    pub fn page_url(&self, base: &str) -> String {
        format!("{}/media/{}", base, self.slug)
    }
}

// ————————————————————————————————————————————————
// Episodio
// ————————————————————————————————————————————————

/// Episodio de un anime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// ID interno del episodio en el sitio (ej: 3433)
    pub id: u32,

    /// Número del episodio (ej: 1, 2, 3...)
    pub number: u32,

    /// Slug del anime al que pertenece.
    /// Necesario para construir la URL del episodio.
    pub anime_slug: String,
}

impl Episode {
    /// Título de visualización en la TUI.
    pub fn display_title(&self) -> String {
        format!("Episodio {}", self.number)
    }
}

// ————————————————————————————————————————————————
// Fuentes de video
// ————————————————————————————————————————————————

/// Tipo de audio de la fuente de video.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AudioType {
    /// Subtitulado (audio original con subtítulos en español)
    Sub,
    /// Doblado (audio en español)
    Dub,
}

impl AudioType {
    pub fn display(&self) -> &str {
        match self {
            AudioType::Sub => "SUB",
            AudioType::Dub => "DUB",
        }
    }
}

/// Una fuente/servidor de video individual.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSource {
    /// Nombre del servidor (ej: "HLS", "Mega", "MP4Upload")
    pub server: String,

    /// URL del embed o stream.
    /// Para HLS: URL directa al reproductor (compatible con mpv)
    /// Para otros: URL de iframe embed (abrir en navegador)
    pub url: String,

    /// Tipo de audio (SUB o DUB)
    pub audio: AudioType,

    /// Calidad de video si está disponible (ej: "1080p", "720p")
    pub quality: Option<String>,
}

impl VideoSource {
    /// Indica si esta fuente tiene soporte de resolución para reproducción en mpv.
    pub fn is_mpv_compatible(&self) -> bool {
        matches!(
            self.server.as_str(),
            "HLS" | "MP4Upload" | "YourUpload" | "PDrain"
        )
    }
}

/// Colección de fuentes de video para un episodio.
/// Agrupa las fuentes por tipo de audio (SUB/DUB).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EpisodeSources {
    /// Fuentes de video subtitulado
    pub sub: Vec<VideoSource>,

    /// Fuentes de video doblado (puede estar vacío)
    pub dub: Vec<VideoSource>,
}

impl EpisodeSources {
    /// Obtiene las fuentes del tipo de audio indicado.
    pub fn get(&self, audio: &AudioType) -> &Vec<VideoSource> {
        match audio {
            AudioType::Sub => &self.sub,
            AudioType::Dub => &self.dub,
        }
    }

    /// Indica si hay fuentes de tipo DUB disponibles.
    pub fn has_dub(&self) -> bool {
        !self.dub.is_empty()
    }

    /// Devuelve la mejor fuente para mpv del tipo de audio indicado.
    /// Prioridad: HLS > PDrain > cualquier otra.
    pub fn best_for_mpv(&self, audio: &AudioType) -> Option<&VideoSource> {
        let sources = self.get(audio);
        sources
            .iter()
            .find(|s| s.server == "HLS")
            .or_else(|| sources.iter().find(|s| s.server == "MP4Upload"))
            .or_else(|| sources.iter().find(|s| s.server == "YourUpload"))
            .or_else(|| sources.iter().find(|s| s.server == "PDrain"))
    }
}

/// Trunca una cadena de forma segura en límites de caracteres UTF-8.
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_source(server: &str, audio: AudioType) -> VideoSource {
        VideoSource {
            server: server.to_string(),
            url: format!("https://example.com/{}", server),
            audio,
            quality: None,
        }
    }

    #[test]
    fn category_maps_ona_and_known_ids() {
        assert_eq!(AnimeCategory::from_id(1), AnimeCategory::TvAnime);
        assert_eq!(AnimeCategory::from_id(2), AnimeCategory::Movie);
        assert_eq!(AnimeCategory::from_id(3), AnimeCategory::Ova);
        assert_eq!(AnimeCategory::from_id(4), AnimeCategory::Special);
        assert_eq!(AnimeCategory::from_id(5), AnimeCategory::Ona);
        assert_eq!(AnimeCategory::Ona.display(), "ONA");
        assert_eq!(AnimeCategory::from_name("ONA"), AnimeCategory::Ona);
        assert_eq!(AnimeCategory::from_id(99), AnimeCategory::Unknown("99".into()));
    }

    #[test]
    fn truncate_chars_is_utf8_safe() {
        let text = "Canción ñandú 日本語";
        assert_eq!(truncate_chars(text, 0), "");
        assert_eq!(truncate_chars(text, 7), "Canción");
        assert_eq!(truncate_chars(text, 10), "Canción ña");
        assert_eq!(truncate_chars(text, 100), text);
    }

    #[test]
    fn video_source_compatibility_and_priority() {
        assert!(make_source("HLS", AudioType::Sub).is_mpv_compatible());
        assert!(make_source("MP4Upload", AudioType::Sub).is_mpv_compatible());
        assert!(make_source("YourUpload", AudioType::Sub).is_mpv_compatible());
        assert!(make_source("PDrain", AudioType::Sub).is_mpv_compatible());
        assert!(!make_source("UPNShare", AudioType::Sub).is_mpv_compatible());
        assert!(!make_source("Mega", AudioType::Sub).is_mpv_compatible());

        let sources = EpisodeSources {
            sub: vec![
                make_source("UPNShare", AudioType::Sub),
                make_source("YourUpload", AudioType::Sub),
                make_source("MP4Upload", AudioType::Sub),
                make_source("HLS", AudioType::Sub),
            ],
            dub: vec![
                make_source("YourUpload", AudioType::Dub),
                make_source("PDrain", AudioType::Dub),
            ],
        };

        assert_eq!(
            sources.best_for_mpv(&AudioType::Sub).map(|s| s.server.as_str()),
            Some("HLS")
        );
        assert_eq!(
            sources.best_for_mpv(&AudioType::Dub).map(|s| s.server.as_str()),
            Some("YourUpload")
        );
    }
}
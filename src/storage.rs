use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::logger;
use crate::structs::{Anime, AnimeCategory};

const APP_DIR_NAME: &str = "ani-tui";
const CONFIG_FILE_NAME: &str = "config.json";
const HISTORY_FILE_NAME: &str = "history.json";

/// Configuración del usuario leída desde `~/.config/ani-tui/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UserConfig {
    /// Reanudar automáticamente desde el minuto guardado en mpv.
    pub resume_playback: bool,
    /// Porcentaje visto mínimo (50..=100) para marcar un episodio como completado.
    pub completed_threshold_percent: u8,
    /// Número máximo de animes conservados en el historial.
    pub max_history_animes: usize,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            resume_playback: true,
            completed_threshold_percent: 90,
            max_history_animes: 100,
        }
    }
}

/// Progreso guardado para un episodio específico.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpisodeProgress {
    pub episode_number: u32,
    pub position_secs: u64,
    #[serde(default)]
    pub duration_secs: Option<u64>,
    #[serde(default)]
    pub completed: bool,
    pub updated_at: u64,
}

/// Entrada de historial de un anime visto recientemente.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnimeHistoryEntry {
    pub slug: String,
    pub title: String,
    pub category: AnimeCategory,
    #[serde(default)]
    pub cover_url: Option<String>,
    pub last_episode: u32,
    pub updated_at: u64,
    #[serde(default)]
    pub episodes: BTreeMap<u32, EpisodeProgress>,
}

/// Historial completo persistido en `~/.config/ani-tui/history.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WatchHistory {
    pub animes: Vec<AnimeHistoryEntry>,
}

impl WatchHistory {
    pub fn get_anime(&self, slug: &str) -> Option<&AnimeHistoryEntry> {
        self.animes.iter().find(|a| a.slug == slug)
    }

    pub fn get_episode(&self, slug: &str, episode_number: u32) -> Option<&EpisodeProgress> {
        self.get_anime(slug)?.episodes.get(&episode_number)
    }

    /// Devuelve el segundo desde donde reanudar un episodio, si aplica.
    pub fn resume_seconds(
        &self,
        slug: &str,
        episode_number: u32,
        config: &UserConfig,
    ) -> Option<u64> {
        if !config.resume_playback {
            return None;
        }
        let ep = self.get_episode(slug, episode_number)?;
        if ep.completed || ep.position_secs < 5 {
            None
        } else {
            Some(ep.position_secs)
        }
    }

    /// Determina qué episodio seleccionar inicialmente al abrir un anime del historial.
    pub fn preferred_episode(&self, slug: &str, available_numbers: &[u32]) -> Option<u32> {
        let entry = self.get_anime(slug)?;
        let last = entry.last_episode;
        let is_done = entry
            .episodes
            .get(&last)
            .is_some_and(|p| p.completed);

        if is_done {
            let next = last.saturating_add(1);
            if available_numbers.contains(&next) {
                return Some(next);
            }
        }

        if available_numbers.contains(&last) {
            Some(last)
        } else {
            None
        }
    }

    /// Registra la apertura de un episodio conservando su minuto previo si existía.
    pub fn record_launch(&mut self, anime: &Anime, episode_number: u32, max_animes: usize) {
        let now = unix_now();
        let entry = self.upsert_anime(anime, episode_number, now, max_animes);
        entry
            .episodes
            .entry(episode_number)
            .and_modify(|ep| ep.updated_at = now)
            .or_insert(EpisodeProgress {
                episode_number,
                position_secs: 0,
                duration_secs: None,
                completed: false,
                updated_at: now,
            });
    }

    /// Actualiza la posición de reproducción observada desde `mpv`.
    /// Devuelve `true` si el cambio merece persistirse en disco.
    pub fn record_progress(
        &mut self,
        slug: &str,
        episode_number: u32,
        position_secs: u64,
        duration_secs: Option<u64>,
        ended_eof: bool,
        config: &UserConfig,
    ) -> bool {
        let Some(pos_idx) = self.animes.iter().position(|a| a.slug == slug) else {
            return false;
        };

        let now = unix_now();
        let entry = &mut self.animes[pos_idx];
        entry.last_episode = episode_number;
        entry.updated_at = now;

        let threshold = f64::from(config.completed_threshold_percent.clamp(50, 100)) / 100.0;
        let prev = entry.episodes.get(&episode_number).cloned();

        let effective_duration = duration_secs.or_else(|| prev.as_ref().and_then(|p| p.duration_secs));
        let completed = ended_eof
            || effective_duration
                .is_some_and(|dur| dur > 30 && (position_secs as f64 / dur as f64) >= threshold);

        let effective_pos = if ended_eof {
            effective_duration.unwrap_or(position_secs)
        } else {
            position_secs
        };

        let significant = match prev {
            None => true,
            Some(ref p) => {
                p.completed != completed
                    || p.duration_secs.is_none() && effective_duration.is_some()
                    || effective_pos.abs_diff(p.position_secs) >= 4
            }
        };

        entry.episodes.insert(
            episode_number,
            EpisodeProgress {
                episode_number,
                position_secs: effective_pos,
                duration_secs: effective_duration,
                completed,
                updated_at: now,
            },
        );

        if pos_idx > 0 {
            let moved = self.animes.remove(pos_idx);
            self.animes.insert(0, moved);
        }

        significant || ended_eof
    }

    pub fn remove_anime(&mut self, slug: &str) -> bool {
        let before = self.animes.len();
        self.animes.retain(|a| a.slug != slug);
        self.animes.len() != before
    }

    fn upsert_anime(
        &mut self,
        anime: &Anime,
        episode_number: u32,
        now: u64,
        max_animes: usize,
    ) -> &mut AnimeHistoryEntry {
        if let Some(pos) = self.animes.iter().position(|a| a.slug == anime.slug) {
            let mut entry = self.animes.remove(pos);
            entry.title = anime.title.clone();
            entry.category = anime.category.clone();
            if anime.cover_url.is_some() {
                entry.cover_url = anime.cover_url.clone();
            }
            entry.last_episode = episode_number;
            entry.updated_at = now;
            self.animes.insert(0, entry);
        } else {
            self.animes.insert(
                0,
                AnimeHistoryEntry {
                    slug: anime.slug.clone(),
                    title: anime.title.clone(),
                    category: anime.category.clone(),
                    cover_url: anime.cover_url.clone(),
                    last_episode: episode_number,
                    updated_at: now,
                    episodes: BTreeMap::new(),
                },
            );
            let limit = max_animes.max(1);
            self.animes.truncate(limit);
        }

        &mut self.animes[0]
    }
}

/// Formatea segundos como `MM:SS` o `H:MM:SS` para mostrar en la TUI.
pub fn format_duration(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

/// Resuelve `~/.config/ani-tui/` respetando `XDG_CONFIG_HOME`.
pub fn resolve_config_dir() -> Option<PathBuf> {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    resolve_config_dir_from(xdg.as_deref(), home.as_deref())
}

fn resolve_config_dir_from(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    if let Some(xdg) = xdg_config_home.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(xdg).join(APP_DIR_NAME));
    }
    home.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|h| PathBuf::from(h).join(".config").join(APP_DIR_NAME))
}

/// Crea `~/.config/ani-tui/` si no existe al abrir la aplicación.
pub fn ensure_config_dir() -> Option<PathBuf> {
    let dir = resolve_config_dir()?;
    if let Err(e) = fs::create_dir_all(&dir) {
        logger::log_warn(
            "storage",
            &format!("no se pudo crear '{}': {}", dir.display(), e),
        );
        return None;
    }
    Some(dir)
}

/// Crea la carpeta de configuración si falta y carga configuración e historial.
/// Si los archivos no existen o están vacíos, los ignora y retorna valores por defecto.
pub fn init_and_load() -> (Option<PathBuf>, UserConfig, WatchHistory) {
    let Some(dir) = ensure_config_dir() else {
        return (None, UserConfig::default(), WatchHistory::default());
    };
    let (config, history) = load_from_dir(&dir);
    (Some(dir), config, history)
}

/// Lee `config.json` y `history.json` desde un directorio dado, tolerando archivos vacíos.
pub fn load_from_dir(dir: &Path) -> (UserConfig, WatchHistory) {
    let config = read_json_or_default::<UserConfig>(&dir.join(CONFIG_FILE_NAME));
    let history = read_json_or_default::<WatchHistory>(&dir.join(HISTORY_FILE_NAME));
    (config, history)
}

fn read_json_or_default<T>(path: &Path) -> T
where
    T: Default + for<'de> Deserialize<'de>,
{
    let raw = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return T::default(),
        Err(e) => {
            logger::log_warn(
                "storage",
                &format!("no se pudo leer '{}': {}", path.display(), e),
            );
            return T::default();
        }
    };

    if raw.trim().is_empty() {
        return T::default();
    }

    match serde_json::from_str::<T>(&raw) {
        Ok(parsed) => parsed,
        Err(e) => {
            logger::log_warn(
                "storage",
                &format!("archivo '{}' inválido ({}), usando valores por defecto", path.display(), e),
            );
            T::default()
        }
    }
}

/// Persiste el historial de forma atómica en `history.json`.
pub fn save_history(dir: &Path, history: &WatchHistory) -> anyhow::Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("No se pudo crear directorio {}", dir.display()))?;

    let target = dir.join(HISTORY_FILE_NAME);
    let tmp = dir.join(format!("{}.tmp.{}", HISTORY_FILE_NAME, std::process::id()));
    let json = serde_json::to_string_pretty(history).context("Error serializando historial")?;

    fs::write(&tmp, json).with_context(|| format!("Error escribiendo {}", tmp.display()))?;
    fs::rename(&tmp, &target).with_context(|| {
        let _ = fs::remove_file(&tmp);
        format!("Error actualizando {}", target.display())
    })?;
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ani_tui_storage_{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_anime(slug: &str, title: &str) -> Anime {
        Anime {
            id: "190".into(),
            title: title.into(),
            slug: slug.into(),
            synopsis: "Sinopsis".into(),
            category: AnimeCategory::TvAnime,
            mal_id: None,
            episodes_count: Some(220),
            score: Some(8.2),
            cover_url: Some("https://cdn.animeav1.com/covers/190.jpg".into()),
        }
    }

    #[test]
    fn resolve_config_dir_prefers_xdg_then_home_dot_config() {
        assert_eq!(
            resolve_config_dir_from(Some("/tmp/xdg"), Some("/home/u")),
            Some(PathBuf::from("/tmp/xdg/ani-tui"))
        );
        assert_eq!(
            resolve_config_dir_from(Some("  "), Some("/home/u")),
            Some(PathBuf::from("/home/u/.config/ani-tui"))
        );
        assert_eq!(resolve_config_dir_from(None, None), None);
    }

    #[test]
    fn load_ignores_missing_empty_and_corrupt_files() {
        let dir = temp_dir("empty");
        let (cfg, hist) = load_from_dir(&dir);
        assert_eq!(cfg, UserConfig::default());
        assert!(hist.animes.is_empty());

        fs::write(dir.join(CONFIG_FILE_NAME), "   \n").unwrap();
        fs::write(dir.join(HISTORY_FILE_NAME), "").unwrap();
        let (cfg2, hist2) = load_from_dir(&dir);
        assert_eq!(cfg2, UserConfig::default());
        assert!(hist2.animes.is_empty());

        fs::write(dir.join(CONFIG_FILE_NAME), "{not json").unwrap();
        let (cfg3, _) = load_from_dir(&dir);
        assert_eq!(cfg3, UserConfig::default());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn records_progress_resumes_and_saves_atomically() {
        let dir = temp_dir("roundtrip");
        let cfg = UserConfig::default();
        let mut hist = WatchHistory::default();
        let naruto = sample_anime("naruto", "Naruto");

        hist.record_launch(&naruto, 5, cfg.max_history_animes);
        assert!(hist.resume_seconds("naruto", 5, &cfg).is_none());

        // Minuto 14:32 sobre 24:00 -> reanudar desde 872s
        assert!(hist.record_progress("naruto", 5, 872, Some(1440), false, &cfg));
        assert_eq!(hist.resume_seconds("naruto", 5, &cfg), Some(872));
        assert_eq!(hist.preferred_episode("naruto", &[1, 5, 6]), Some(5));

        save_history(&dir, &hist).unwrap();
        let (_, loaded) = load_from_dir(&dir);
        assert_eq!(loaded.resume_seconds("naruto", 5, &cfg), Some(872));

        // Al pasar el umbral de completado (>=90%) se marca visto y sugiere ep 6
        assert!(hist.record_progress("naruto", 5, 1320, Some(1440), false, &cfg));
        assert!(hist.get_episode("naruto", 5).unwrap().completed);
        assert_eq!(hist.resume_seconds("naruto", 5, &cfg), None);
        assert_eq!(hist.preferred_episode("naruto", &[1, 5, 6]), Some(6));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn formats_timestamps_with_and_without_hours() {
        assert_eq!(format_duration(0), "00:00");
        assert_eq!(format_duration(872), "14:32");
        assert_eq!(format_duration(3725), "1:02:05");
    }
}

use std::collections::{HashMap, VecDeque};
use std::io::IsTerminal;

use anyhow::Result;
use image::{DynamicImage, Rgba};
use ratatui_image::picker::Picker;

use crate::{logger, scraper, security};

/// Límite máximo de tamaño para descargar una portada (5 MiB).
const MAX_COVER_BYTES: usize = 5 * 1024 * 1024;

/// Máximo de portadas decodificadas mantenidas en caché en memoria.
const MAX_CACHE_ENTRIES: usize = 32;

/// Fondo por defecto coincidente con `ui::search::palette::BG`.
const COVER_BG_RGBA: Rgba<u8> = Rgba([10, 10, 20, 255]);

/// Estado en caché de una portada por URL.
pub enum CoverEntry {
    Loading,
    Ready(DynamicImage),
    Failed,
}

/// Caché en memoria con cota máxima para portadas descargadas.
#[derive(Default)]
pub struct CoverCache {
    entries: HashMap<String, CoverEntry>,
    order: VecDeque<String>,
}

impl CoverCache {
    pub fn get(&self, url: &str) -> Option<&CoverEntry> {
        self.entries.get(url)
    }

    pub fn mark_loading(&mut self, url: &str) {
        if !self.entries.contains_key(url) {
            self.evict_if_full();
            self.order.push_back(url.to_string());
        }
        self.entries.insert(url.to_string(), CoverEntry::Loading);
    }

    pub fn insert_ready(&mut self, url: String, image: DynamicImage) {
        self.insert_entry(url, CoverEntry::Ready(image));
    }

    pub fn insert_failed(&mut self, url: String) {
        self.insert_entry(url, CoverEntry::Failed);
    }

    fn insert_entry(&mut self, url: String, entry: CoverEntry) {
        if let Some(pos) = self.order.iter().position(|k| k == &url) {
            self.order.remove(pos);
        } else {
            self.evict_if_full();
        }
        self.order.push_back(url.clone());
        self.entries.insert(url, entry);
    }

    fn evict_if_full(&mut self) {
        let mut remaining = self.order.len();
        while self.entries.len() >= MAX_CACHE_ENTRIES && remaining > 0 {
            remaining -= 1;
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if matches!(self.entries.get(&oldest), Some(CoverEntry::Loading)) {
                self.order.push_back(oldest);
                continue;
            }
            self.entries.remove(&oldest);
        }
    }
}

/// Picker sin E/S de terminal (seguro para tests y entornos no interactivos).
pub fn default_picker() -> Picker {
    let mut picker = Picker::from_fontsize((10, 20));
    picker.set_background_color(COVER_BG_RGBA);
    picker
}

/// Detecta el protocolo gráfico soportado por la terminal tras entrar en pantalla alterna.
pub fn detect_picker() -> Picker {
    let interactive = std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::env::var("TERM").ok().as_deref() != Some("dumb");

    let mut picker = if interactive {
        match Picker::from_query_stdio() {
            Ok(p) => p,
            Err(e) => {
                logger::log_warn("cover", &format!("no se pudo consultar protocolo de terminal: {}", e));
                default_picker()
            }
        }
    } else {
        default_picker()
    };

    picker.set_background_color(COVER_BG_RGBA);
    logger::log_info(
        "cover",
        &format!(
            "protocolo gráfico: {:?}, celda: {:?}",
            picker.protocol_type(),
            picker.font_size()
        ),
    );
    picker
}

/// Descarga y decodifica una portada de anime desde su URL validada.
pub async fn fetch_cover(url: &str) -> Result<DynamicImage> {
    security::validate_cover_url(url)
        .map_err(|e| anyhow::anyhow!("URL de portada inválida: {}", e))?;

    let bytes = scraper::client::fetch_bytes(url, MAX_COVER_BYTES).await?;
    let image = tokio::task::spawn_blocking(move || image::load_from_memory(&bytes))
        .await
        .map_err(|e| anyhow::anyhow!("Fallo al esperar decodificación de portada: {}", e))??;

    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn dummy_img() -> DynamicImage {
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(4, 4, Rgb([100, 200, 255])))
    }

    #[test]
    fn cache_evicts_oldest_entries_beyond_limit() {
        let mut cache = CoverCache::default();
        for i in 0..=(MAX_CACHE_ENTRIES + 4) {
            cache.insert_ready(format!("https://cdn.animeav1.com/covers/{}.jpg", i), dummy_img());
        }
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        assert!(cache.get("https://cdn.animeav1.com/covers/0.jpg").is_none());
        assert!(cache
            .get(&format!(
                "https://cdn.animeav1.com/covers/{}.jpg",
                MAX_CACHE_ENTRIES + 4
            ))
            .is_some());
    }

    #[test]
    fn cache_keeps_loading_entries_during_eviction() {
        let mut cache = CoverCache::default();
        let loading_url = "https://cdn.animeav1.com/covers/loading.jpg".to_string();
        cache.mark_loading(&loading_url);
        for i in 0..MAX_CACHE_ENTRIES {
            cache.insert_ready(format!("https://cdn.animeav1.com/covers/{}.jpg", i), dummy_img());
        }
        assert!(matches!(cache.get(&loading_url), Some(CoverEntry::Loading)));
    }
}

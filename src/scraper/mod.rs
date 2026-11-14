// ============================================================
// src/scraper/mod.rs — Módulo de scraping
// ============================================================
//
// Punto de entrada público del módulo de scraping.
// Re-exporta las funciones principales de cada sub-módulo
// para un acceso más conveniente desde el resto de la aplicación.
//
// Estructura del módulo:
//   scraper::client   - Cliente HTTP configurado
//   scraper::search   - Búsqueda de anime en el catálogo
//   scraper::episodes - Obtención de episodios de un anime
//   scraper::sources  - Obtención de fuentes de video de un episodio
// ============================================================

pub mod client;
pub mod episodes;
pub mod search;
pub mod sources;

// Re-exportaciones convenientes para el uso desde otros módulos
pub use search::search_anime;
pub use episodes::get_episodes;
#[cfg(feature = "serve")]
pub use episodes::get_episodes_by_slug;
pub use sources::get_video_sources;

/// Busca `marker` (que debe terminar con el delimitador `open`, ej. `"results:["`
/// o `"embeds:{"`) en `text` y extrae el slice balanceado hasta su `close`
/// correspondiente, ignorando delimitadores dentro de literales de cadena JS.
pub(crate) fn find_balanced_content<'a>(
    text: &'a str,
    marker: &str,
    open: u8,
    close: u8,
) -> Option<&'a str> {
    let start = find_marker_start(text, marker)?;
    let bytes = text.as_bytes();
    let mut depth = 1usize;
    let mut in_quote: Option<u8> = None;
    let mut escaped = false;

    for (offset, &b) in bytes[start..].iter().enumerate() {
        if in_quote.is_some() {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if Some(b) == in_quote {
                in_quote = None;
            }
            continue;
        }

        if b == b'"' || b == b'\'' {
            in_quote = Some(b);
        } else if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(&text[start..start + offset]);
            }
        }
    }

    None
}

/// Localiza el primer `marker` precedido por inicio de texto o separador válido.
fn find_marker_start(text: &str, marker: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut pos = 0;

    while let Some(rel) = text[pos..].find(marker) {
        let abs = pos + rel;
        let valid_boundary = abs == 0
            || (!bytes[abs - 1].is_ascii_alphanumeric()
                && bytes[abs - 1] != b'_'
                && bytes[abs - 1] != b'$');
        if valid_boundary {
            return Some(abs + marker.len());
        }
        pos = abs + marker.len();
    }

    None
}

/// Itera objetos JS de primer nivel (`{...}`) dentro de un bloque de array,
/// devolviendo slices sin asignar copias y respetando comillas y objetos anidados.
pub(crate) fn iter_top_level_objects(block: &str) -> Vec<&str> {
    let bytes = block.as_bytes();
    let mut objects = Vec::new();
    let mut depth = 0usize;
    let mut obj_start: Option<usize> = None;
    let mut in_quote: Option<u8> = None;
    let mut escaped = false;

    for (i, &b) in bytes.iter().enumerate() {
        if in_quote.is_some() {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if Some(b) == in_quote {
                in_quote = None;
            }
            continue;
        }

        if b == b'"' || b == b'\'' {
            in_quote = Some(b);
        } else if b == b'{' {
            if depth == 0 {
                obj_start = Some(i);
            }
            depth += 1;
        } else if b == b'}' && depth > 0 {
            depth -= 1;
            if depth == 0 {
                if let Some(start) = obj_start.take() {
                    objects.push(&block[start..=i]);
                }
            }
        }
    }

    objects
}

/// Desescapa secuencias básicas de literales JavaScript en una sola pasada.
pub(crate) fn unescape_js_string(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_string();
    }

    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

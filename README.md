# 🎌 ani-tui

> **TUI interactiva para buscar y reproducir anime sin anuncios**

Aplicación de terminal (TUI) escrita en Rust que permite buscar anime en `animeav1.com` y reproducirlo directamente con `mpv` o en el navegador — sin anuncios, sin Chrome, sin Electron.

---

## Características

-  **Búsqueda de anime** con resultados paginados
-  **Lista de episodios** con soporte para series largas (One Piece: 1166+ eps)
-  **Múltiples servidores**: HLS, Mega, MP4Upload, UPNShare, PixelDrain, TeraBox
-  **SUB y DUB** cuando están disponibles
-  **mpv** como reproductor principal (sin anuncios)
-  **Navegador** como fallback automático
-  **Sin headless Chrome** — scraping directo del HTML (≈10x más rápido)
-  **TUI moderna** con paleta cyberpunk y animaciones

---

## Instalación

### Requisitos

- Rust 1.70+ (con Cargo)
- `mpv` (opcional, pero recomendado para reproducción automatica)

```bash
# Clonar el repositorio
git clone https://github.com/glimp/ani-tui
cd ani-tui

# Compilar en modo release
cargo build --release

# Instalar en el sistema (opcional)
sudo cp target/release/ani-tui /usr/local/bin/
```

---

## Uso

### TUI interactiva (por defecto)

```bash
ani-tui
# o
ani-tui tui
```

### Búsqueda sin TUI (texto plano)

```bash
ani-tui search "one piece"
ani-tui search "naruto" --page 2
```

### Servidor API REST (modo legacy, antes del cambio de enfoque)

```bash
ani-tui serve
ani-tui serve --port 8080

# Endpoints disponibles:
# GET /search?q=naruto&page=1
# GET /anime/one-piece/episodes
# GET /episode/serial-experiments-lain/1/sources
```

---

## ⌨️ Keybindings de la TUI

| Tecla | Acción |
|-------|--------|
| `↑` / `↓` | Navegar en listas |
| `Enter` | Confirmar selección |
| `Esc` / `q` | Volver / salir |
| `Tab` | Cambiar entre SUB y DUB |
| `s` / `d` | Ir directamente a SUB / DUB |
| `/` | Nueva búsqueda (desde cualquier pantalla) |
| `PgUp` / `PgDn` | Saltar 10 items |
| `Home` / `End` | Inicio / fin de lista |
| `←` / `→` | Página anterior / siguiente de resultados |
| `Ctrl+U` | Borrar campo de búsqueda |
| `?` | Mostrar ayuda |

---

##Arquitectura

```
src/
├── main.rs          # Entry point: CLI (clap), event loop TUI
├── app.rs           # Estado central: Screen, AppState, ListState
├── config.rs        # Constantes: BASE_URL, USER_AGENT, timeouts
├── structs.rs       # Tipos: Anime, Episode, EpisodeSources, AudioType
├── player.rs        # Reproductor: mpv → navegador fallback
├── scraper/
│   ├── client.rs    # Cliente HTTP (reqwest, sin Chrome o navegador headless)
│   ├── search.rs    # Búsqueda: extrae JSON embebido del HTML
│   ├── episodes.rs  # Episodios: extrae episodes:[{id,number}] del HTML
│   └── sources.rs   # Fuentes: extrae embeds:{SUB,DUB} del HTML
├── ui/
│   ├── search.rs    # Pantalla: búsqueda + paleta de colores
│   ├── results.rs   # Pantalla: lista de animes con sinopsis
│   ├── episodes.rs  # Pantalla: lista de episodios
│   ├── sources.rs   # Pantalla: servidores SUB/DUB
│   └── help.rs      # Modal: ayuda con keybindings
└── routes.rs        # API REST endpoints (feature "serve")
```

---

## html_paginas/

Carpeta con capturas HTML reales del sitio para referencia y debugging.
Ver [html_paginas/README.md](html_paginas/README.md) para documentación
detallada de la estructura de datos del sitio.

---

## Por qué sin headless Chrome

El sitio `animeav1.com` usa **Nuxt.js SSR** (Server-Side Rendering), lo que significa
que todos los datos están en el HTML inicial como JSON embebido. No se necesita
ejecutar JavaScript para acceder a:

- Resultados de búsqueda: `results:[{id,title,slug,...}]`
- Lista de episodios: `episodes:[{id,number}]`
- Fuentes de video: `embeds:{SUB:[{server,url}],DUB:[...]}`

Esto hace el scraping ≈10x más rápido y elimina la dependencia de
`chromedriver`, `chromium-browser` y otras herramientas externas.

---

## Licencia

GPL-3.0 — Ver [LICENSE](LICENSE)

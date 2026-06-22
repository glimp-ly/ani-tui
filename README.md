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

### Dependencias

| Dependencia | Para qué | Requerida |
|-------------|----------|-----------|
| `curl` | Descargar releases de GitHub (`--release`) | Solo para `--release` |
| `cargo` (Rust ≥ 1.70) | Compilar desde fuente | Solo para compilar |
| `mpv` | Reproducción directa de video sin anuncios | Opcional (recomendado) |
| `xdg-utils` | Abrir URLs en el navegador del sistema (fallback) | Opcional |

Para instalar las dependencias:

```bash
# Arch Linux
sudo pacman -S curl rust mpv xdg-utils

# Debian / Ubuntu
sudo apt install curl cargo mpv xdg-utils

# Fedora
sudo dnf install curl cargo mpv xdg-utils
```

> `mpv` y `xdg-utils` son opcionales: sin ellos, ani-tui seguirá funcionando
> pero no podrá reproducir video directamente.

---

### Método 1 — Script de instalación (recomendado)

```bash
# Clonar el repositorio
git clone https://github.com/glimp-ly/ani-tui
cd ani-tui

# Dar permisos de ejecución al script
chmod +x install.sh

# Instalar compilando desde el código fuente (requiere cargo)
./install.sh

# — O — descargar e instalar el binario precompilado del último release
./install.sh --release

# Instalar en un directorio sin sudo (ej. ~/.local/bin)
PREFIX=~/.local ./install.sh --release
```

El script detecta automáticamente si necesita `sudo` según el directorio de destino
(`/usr/local/bin` por defecto). Con `PREFIX=~/.local` se instala en `~/.local/bin`
sin necesidad de privilegios.

---

### Método 2 — Manual (desde el código fuente)

```bash
# Clonar el repositorio
git clone https://github.com/glimp-ly/ani-tui
cd ani-tui

# Compilar en modo release
cargo build --release

# Instalar en el sistema
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

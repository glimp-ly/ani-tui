# 📄 Documentación de Páginas HTML — animeav1.com

> **Propósito de esta carpeta**: Documentar la estructura HTML y los datos embebidos
> del sitio `animeav1.com` para facilitar el mantenimiento del scraper. Los archivos
> `.html` aquí presentes son capturas reales del sitio usadas como referencia y para
> pruebas sin necesidad de conexión a internet.
>
> **Última actualización**: Junio 2026
> **Sitio objetivo**: https://animeav1.com
> **Tecnología del sitio**: Nuxt.js (SSR/SSG) — los datos se envían como JSON embebido
>   en el HTML dentro de la etiqueta `<script id="__NUXT_DATA__">`.

---

## 📁 Índice de archivos

| Archivo | URL capturada | Descripción |
|---------|--------------|-------------|
| `catalogo_search_one_piece.html` | `/catalogo?search=one+piece` | Resultados de búsqueda para "one piece" |
| `catalogo_search_naruto.html` | `/catalogo?search=naruto` | Resultados de búsqueda para "naruto" |
| `catalogo_search_lain.html` | `/catalogo?search=serial+experiments+lain` | Resultados para "serial experiments lain" |
| `media_one_piece.html` | `/media/one-piece` | Página principal del anime One Piece |
| `media_naruto.html` | `/media/naruto` | Página principal del anime Naruto |
| `media_serial_experiments_lain.html` | `/media/serial-experiments-lain` | Página principal de Serial Experiments Lain |
| `media_one_piece_ep1.html` | `/media/one-piece/1` | Episodio 1 de One Piece (con fuentes de video) |
| `media_serial_lain_ep1.html` | `/media/serial-experiments-lain/1` | Episodio 1 de Lain (con fuentes SUB y DUB) |

---

## 🔍 Estructura de datos por tipo de página

### 1. Página de Búsqueda (`/catalogo?search=<query>`)

**URL de ejemplo**: `https://animeav1.com/catalogo?search=one+piece`

**Cómo funciona**: El sitio es una SPA Nuxt.js con SSR. El servidor inyecta los datos
directamente en el HTML como un array serializado en un `<script>` especial. No se
necesita JavaScript para acceder a estos datos — están en el HTML estático.

**Dónde encontrar los datos**: Buscar el patrón `results:[` dentro del HTML. Los datos
de búsqueda están embebidos como JavaScript literal (no JSON estricto: sin comillas
en las keys, usa `void 0` en lugar de `null`).

#### Estructura del bloque `results`

```javascript
// Patrón en el HTML (fragmento real del sitio):
results:[
  {
    id: "197",                    // ID numérico como string
    title: "One Piece",           // Título del anime
    synopsis: "Apenas sobreviviendo en un barril...",  // Sinopsis completa
    categoryId: 1,                // ID de categoría (ver tabla abajo)
    slug: "one-piece",            // Identificador URL (usar para navegar)
    category: {                   // Objeto de categoría expandido
      id: 1,
      name: "TV Anime",
      slug: "tv-anime",
      malId: 1                    // ID en MyAnimeList.net
    }
  },
  // ... más resultados
]
```

#### Tabla de `categoryId` (tipos de anime)

| categoryId | Nombre | Descripción |
|-----------|--------|-------------|
| `1` | TV Anime | Series de televisión regulares |
| `2` | Movie | Películas |
| `3` | OVA | Original Video Animation |
| `4` | Special | Episodios especiales |

#### Metadatos de paginación (al final del bloque)

```javascript
// También presente en el HTML:
pagination: {
  currentPage: 1,
  recordsPerPage: 20,    // Máximo 20 resultados por página
  totalPages: 3,         // Total de páginas para la búsqueda
  totalRecords: 53       // Total de resultados
}
```

#### Regex para extraer resultados (Rust)

```rust
// Extrae el bloque completo de results
let re = Regex::new(r"results:\[(\{.*?\})\]").unwrap();

// Extrae campos individuales de cada resultado
// NOTA: El sitio usa JS literal, no JSON estricto. Procesar campo a campo:
let title_re = Regex::new(r#"title:"([^"]+)""#).unwrap();
let slug_re  = Regex::new(r#"slug:"([^"]+)""#).unwrap();
let id_re    = Regex::new(r#"id:"(\d+)""#).unwrap();
let cat_re   = Regex::new(r#"categoryId:(\d+)"#).unwrap();
```

---

### 2. Página de Anime (`/media/<slug>`)

**URL de ejemplo**: `https://animeav1.com/media/one-piece`

**Propósito**: Obtiene metadatos del anime y la lista de IDs de episodios.

**Importante**: Esta página **NO** contiene las fuentes de video. Solo lista los
episodios disponibles con su ID interno y número. Para las fuentes, ir a la
página de episodio específico.

#### Datos disponibles en el HTML

```javascript
// Patrón real encontrado en el HTML (fragmento de One Piece):
{
  episodesCount: 1166,           // Total de episodios (número entero)
  score: 8.73,                   // Puntuación del anime
  votes: 1445355,                // Número de votos
  slug: "one-piece",             // Slug del anime
  malId: 21,                     // ID en MyAnimeList.net
  seasons: null,                 // null si no tiene temporadas separadas
  createdAt: "2025-02-22 07:34:09.266305+00",
  updatedAt: "2025-06-22 06:34:36.758105+00",
  category: {
    id: 1,
    name: "TV Anime",
    slug: "tv-anime",
    malId: 1
  },
  // Lista de episodios — SOLO ID Y NÚMERO, sin URL ni título propio:
  episodes: [
    { id: 3433, number: 1 },
    { id: 3434, number: 2 },
    { id: 3435, number: 3 },
    // ... hasta episodesCount
  ]
}
```

#### Patrón para construir URL de episodio

```
// La URL del episodio se construye con el SLUG del anime y el NÚMERO:
https://animeav1.com/media/{slug}/{number}

// Ejemplo:
https://animeav1.com/media/one-piece/1      // Episodio 1
https://animeav1.com/media/one-piece/1166   // Episodio 1166
```

> ⚠️ **Nota**: El campo `id` numérico del episodio (ej: `3433`) NO se usa en la URL.
> La URL se construye con el `slug` del anime y el `number` del episodio.

#### Regex para extraer episodios (Rust)

```rust
// Extrae el bloque de episodios
let ep_block_re = Regex::new(r"episodes:\[([^\]]+)\]").unwrap();

// Extrae pares id/number de cada episodio
let ep_re = Regex::new(r"\{id:(\d+),number:(\d+)\}").unwrap();

// Extrae metadatos del anime
let count_re  = Regex::new(r"episodesCount:(\d+)").unwrap();
let score_re  = Regex::new(r"score:([\d.]+)").unwrap();
let slug_re   = Regex::new(r#"slug:"([^"]+)""#).unwrap();
```

---

### 3. Página de Episodio (`/media/<slug>/<number>`)

**URL de ejemplo**: `https://animeav1.com/media/one-piece/1`

**Propósito**: Obtiene todas las fuentes de video (embeds) y enlaces de descarga
para un episodio específico, tanto en subtitulado (SUB) como en doblado (DUB).

**Este es el dato más valioso**: Contiene URLs directas a reproductores externos
como HLS (m3u8), Mega, MP4Upload, UPNShare, TeraBox, PixelDrain, etc.

#### Estructura del bloque `embeds`

```javascript
// Patrón real encontrado en el HTML (fragmento de Serial Experiments Lain Ep.1):
{
  embeds: {
    // Versión subtitulada (Sub Español)
    SUB: [
      { server: "HLS",      url: "https://player.zilla-networks.com/play/a351f2..." },
      { server: "UPNShare", url: "https://animeav1.uns.bio/#tauy6t" },
      { server: "Mega",     url: "https://mega.nz/embed/XYt1VYjL#2sivF5uJvE..." },
      { server: "MP4Upload",url: "https://www.mp4upload.com/embed-vvgjuxkynbmv.html" }
    ],
    // Versión doblada (Doblaje Español) — no siempre disponible
    DUB: [
      { server: "HLS",      url: "https://player.zilla-networks.com/play/3570e9..." },
      { server: "PDrain",   url: "https://pixeldrain.com/u/MJzY7Ab1?embed" },
      { server: "UPNShare", url: "https://animeav1.uns.bio/#xi5nuo" },
      { server: "Mega",     url: "https://mega.nz/embed/RU50AYQY#FET5pEtERREaC9..." },
      { server: "MP4Upload",url: "https://www.mp4upload.com/embed-vesque70ioze.html" }
    ]
  },
  // Enlaces de descarga directa (sin embed)
  downloads: {
    SUB: [
      { server: "Mega",     url: "https://mega.nz/file/D3hnjLDb#w5awpN-nEWDZX..." },
      { server: "MP4Upload",url: "https://www.mp4upload.com/78kptpyotrle" },
      { server: "1Fichier", url: "https://1fichier.com/?323mzyoizx3hsi9ol366" }
    ],
    DUB: [
      { server: "PDrain",   url: "https://pixeldrain.com/u/MJzY7Ab1" }
    ]
  }
}
```

#### Tabla de servidores disponibles

| Servidor | Tipo | Compatibilidad con `mpv` | Notas |
|---------|------|--------------------------|-------|
| `HLS` | Stream m3u8 | ✅ Excelente | Mejor opción para mpv. URL directa a player.zilla-networks.com |
| `Mega` | Embed | ⚠️ Difícil | Requiere login para archivos grandes; embed funciona en browser |
| `MP4Upload` | Embed | ⚠️ Difícil | Embed iframe; mejor abrir en browser |
| `UPNShare` | Embed | ⚠️ Difícil | Servidor dedicado del sitio |
| `TeraBox` | Embed | ❌ No | Requiere autenticación |
| `PDrain` (PixelDrain) | Directo | ✅ Bueno | URL directa accesible |

> 💡 **Estrategia recomendada para reproducción**:
> 1. Preferir servidor `HLS` → abrir con `mpv <url>`
> 2. Si HLS no disponible, intentar `PDrain` → abrir con `mpv`
> 3. Fallback: abrir la URL del embed en el navegador del sistema

#### Regex para extraer embeds (Rust)

```rust
// Extrae el bloque completo de embeds
let embeds_re = Regex::new(r"embeds:\{(.*?)\},downloads:").unwrap();

// Extrae el bloque SUB o DUB dentro de embeds
let sub_re = Regex::new(r"SUB:\[([^\]]+)\]").unwrap();
let dub_re = Regex::new(r"DUB:\[([^\]]+)\]").unwrap();

// Extrae pares server/url de cada fuente
let source_re = Regex::new(
    r#"\{server:"([^"]+)",url:"([^"]+)"\}"#
).unwrap();
```

---

## 🏗️ Cómo se estructura el sitio internamente

El sitio usa **Nuxt.js** (framework Vue.js con SSR). El mecanismo de hidratación
inyecta los datos del servidor en el HTML mediante un bloque especial:

```html
<!-- Fragmento del HTML final del sitio (simplificado): -->
<script id="__NUXT_DATA__" type="application/json">
  [... array comprimido de datos ...]
</script>

<!-- Y también como llamada a función inline (formato alternativo): -->
<script>
  (function(a,b,c,d) {
    window.__NUXT__ = {
      data: {
        results: [...],
        episodes: [...],
        embeds: {...}
      }
    }
  })({},{},{},{})
</script>
```

> ⚠️ **Importante para mantenimiento**: Si el sitio cambia de versión de Nuxt o
> modifica su estructura de hidratación, los patrones regex pueden necesitar
> actualización. Ante cambios en el scraping, **actualizar primero los archivos
> HTML en esta carpeta** con capturas nuevas y ajustar los patrones.

---

## 🧪 Comandos útiles para inspección

```bash
# Capturar una página de búsqueda nueva
curl -s "https://animeav1.com/catalogo?search=QUERY" \
  -A "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36" \
  -o html_paginas/catalogo_search_QUERY.html

# Capturar página de anime
curl -s "https://animeav1.com/media/SLUG" \
  -A "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36" \
  -o html_paginas/media_SLUG.html

# Capturar página de episodio
curl -s "https://animeav1.com/media/SLUG/NUMERO" \
  -A "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36" \
  -o html_paginas/media_SLUG_epNUMERO.html

# Inspeccionar datos embebidos de resultados de búsqueda
grep -o 'slug:"[^"]*"' html_paginas/catalogo_search_QUERY.html

# Ver el bloque de episodios
python3 -c "
import re
with open('html_paginas/media_SLUG.html') as f: c = f.read()
idx = c.find('episodes:')
print(c[idx:idx+500])
"

# Ver los embeds de un episodio
python3 -c "
import re
with open('html_paginas/media_SLUG_epN.html') as f: c = f.read()
idx = c.find('embeds:')
print(c[idx:idx+800])
"
```

---

## 📝 Notas de mantenimiento

1. **Si los resultados de búsqueda están vacíos**: Verificar que el patrón `results:[`
   siga presente. Capturar una nueva página y comparar estructura.

2. **Si los episodios no cargan**: Verificar que `episodes:[{id:N,number:M}]` siga
   siendo el formato. El sitio podría cambiar a `{id:N,num:M}` u otro campo.

3. **Si las fuentes de video fallan**: Verificar el bloque `embeds:{SUB:[...],DUB:[...]}`.
   El servidor HLS de `zilla-networks.com` puede cambiar de dominio.

4. **El User-Agent es importante**: Sin un User-Agent de Chrome, el sitio puede devolver
   respuestas vacías o bloqueadas. Mantener el UA actualizado.

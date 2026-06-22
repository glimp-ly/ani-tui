#!/usr/bin/env bash
# install.sh — Instalador de ani-tui
# Uso:
#   ./install.sh            → compilar e instalar desde el repo local
#   ./install.sh --release  → descargar e instalar el binario del último release de GitHub
#   ./install.sh --help     → mostrar ayuda

set -euo pipefail

# ─── Constantes ────────────────────────────────────────────────
BINARY="ani-tui"
REPO="glimp-ly/ani-tui"
INSTALL_DIR="${PREFIX:-/usr/local}/bin"
GITHUB_API="https://api.github.com/repos/${REPO}/releases/latest"

# ─── Colores (solo si el terminal los soporta) ─────────────────
if [ -t 1 ] && command -v tput &>/dev/null && tput colors &>/dev/null; then
    C_BOLD=$(tput bold)
    C_GREEN=$(tput setaf 2)
    C_YELLOW=$(tput setaf 3)
    C_RED=$(tput setaf 1)
    C_CYAN=$(tput setaf 6)
    C_RESET=$(tput sgr0)
else
    C_BOLD="" C_GREEN="" C_YELLOW="" C_RED="" C_CYAN="" C_RESET=""
fi

info()    { printf "%s==>%s %s%s\n" "${C_GREEN}${C_BOLD}" "${C_RESET}" "$*" "${C_RESET}"; }
warn()    { printf "%s[!]%s %s%s\n" "${C_YELLOW}" "${C_RESET}" "$*" "${C_RESET}"; }
die()     { printf "%s[✗]%s %s%s\n" "${C_RED}" "${C_RESET}" "$*" "${C_RESET}" >&2; exit 1; }
step()    { printf "%s  →%s %s\n"   "${C_CYAN}" "${C_RESET}" "$*"; }

# ─── Ayuda ─────────────────────────────────────────────────────
usage() {
    cat <<EOF
${C_BOLD}ani-tui installer${C_RESET}

Uso: $0 [OPCIÓN]

Opciones:
  (sin args)   Compilar e instalar desde el repo local usando Cargo
  --release    Descargar e instalar el binario del último GitHub release
  --help       Mostrar esta ayuda

Destino de instalación: ${INSTALL_DIR}
  Puede cambiarse con la variable PREFIX:  PREFIX=~/.local ./install.sh

Dependencias opcionales:
  mpv          Reproductor de video (recomendado para reproducción sin anuncios)
  xdg-utils    Para abrir URLs en el navegador (fallback)
EOF
}

# ─── Verificar comando disponible ──────────────────────────────
need() {
    command -v "$1" &>/dev/null || die "Se requiere '$1' pero no está instalado."
}

# ─── Detectar arquitectura para los releases ───────────────────
detect_arch() {
    local machine
    machine=$(uname -m)
    case "$machine" in
        x86_64)          echo "x86_64-unknown-linux-gnu" ;;
        aarch64|arm64)   echo "aarch64-unknown-linux-gnu" ;;
        *)               die "Arquitectura no soportada: $machine" ;;
    esac
}

# ─── Determinar si se requiere sudo para escribir en INSTALL_DIR ───
needs_sudo() {
    [ ! -w "$INSTALL_DIR" ]
}

# ─── Instalar el binario (con o sin sudo) ──────────────────────
install_binary() {
    local src="$1"
    local dest="${INSTALL_DIR}/${BINARY}"

    mkdir -p "$INSTALL_DIR" 2>/dev/null || true

    if needs_sudo; then
        step "Instalando en ${dest} (se requerirá contraseña)..."
        sudo install -Dm755 "$src" "$dest"
    else
        install -Dm755 "$src" "$dest"
    fi
}

# ═══════════════════════════════════════════════════════════════
# MODO 1: Compilar desde el repo local
# ═══════════════════════════════════════════════════════════════
install_from_source() {
    info "Instalando ${BINARY} desde el código fuente..."

    need cargo

    # Verificar que estamos en el directorio correcto
    [ -f Cargo.toml ] || die "Ejecuta el script desde la raíz del repositorio."
    grep -q "name = \"${BINARY}\"" Cargo.toml || die "Este no parece ser el repositorio de ${BINARY}."

    step "Compilando en modo release (esto puede tardar unos minutos)..."
    cargo build --release --quiet

    local built="target/release/${BINARY}"
    [ -f "$built" ] || die "La compilación falló: no se encontró el binario."

    install_binary "$built"
    info "✓ ${BINARY} instalado en ${INSTALL_DIR}/${BINARY}"
    "${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || true
}

# ═══════════════════════════════════════════════════════════════
# MODO 2: Descargar el binario del último GitHub release
# ═══════════════════════════════════════════════════════════════
install_from_release() {
    info "Instalando/actualizando ${BINARY} desde el último GitHub release..."

    need curl

    # Obtener metadata del release
    step "Consultando el último release..."
    local release_json
    release_json=$(curl -fsSL "$GITHUB_API") \
        || die "No se pudo conectar a la API de GitHub. Verifica tu conexión."

    # Extraer versión (sin jq)
    local version
    version=$(printf '%s' "$release_json" | grep '"tag_name"' | head -1 \
              | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
    [ -n "$version" ] || die "No se pudo determinar la versión del release."

    # Verificar si ya está instalada la misma versión
    if command -v "$BINARY" &>/dev/null; then
        local current
        current=$("$BINARY" --version 2>/dev/null | grep -oP '\d+\.\d+\.\d+' | head -1 || true)
        local remote="${version#v}"
        if [ -n "$current" ] && [ "$current" = "$remote" ]; then
            info "Ya tienes la versión más reciente: ${version}"
            exit 0
        fi
        [ -n "$current" ] && step "Actualizando ${current} → ${remote}..."
    fi

    local arch
    arch=$(detect_arch)

    # Nombre del asset esperado en el release
    local asset_name="${BINARY}-${arch}.tar.gz"

    # Extraer URL del asset desde el JSON (sin jq)
    local download_url
    download_url=$(printf '%s' "$release_json" \
        | grep '"browser_download_url"' \
        | grep "$asset_name" \
        | head -1 \
        | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/')

    # Fallback: intentar con binario crudo (sin extensión)
    if [ -z "$download_url" ]; then
        local asset_raw="${BINARY}-${arch}"
        download_url=$(printf '%s' "$release_json" \
            | grep '"browser_download_url"' \
            | grep "$asset_raw" \
            | grep -v '\.tar\.gz\|\.zip\|\.sha' \
            | head -1 \
            | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/')
    fi

    [ -n "$download_url" ] || die "No se encontró un binario para tu arquitectura (${arch}) en el release ${version}."

    step "Descargando ${version} para ${arch}..."

    # Directorio temporal con limpieza garantizada al salir
    local tmpdir
    tmpdir=$(mktemp -d)
    trap 'rm -rf "$tmpdir"' EXIT

    local tmpfile="${tmpdir}/${BINARY}"
    curl -fsSL --progress-bar -o "${tmpfile}.download" "$download_url"

    # Detectar si es tarball o binario directo
    local mime
    mime=$(file -b "${tmpfile}.download" 2>/dev/null || true)

    if printf '%s' "$mime" | grep -qi 'gzip\|tar'; then
        step "Extrayendo tarball..."
        tar -xzf "${tmpfile}.download" -C "$tmpdir"
        local extracted
        extracted=$(find "$tmpdir" -maxdepth 3 -type f -name "$BINARY" | head -1)
        [ -n "$extracted" ] || die "No se encontró '${BINARY}' dentro del tarball."
        mv "$extracted" "$tmpfile"
    else
        mv "${tmpfile}.download" "$tmpfile"
    fi

    chmod +x "$tmpfile"

    # Verificar que el binario funciona antes de instalarlo
    "$tmpfile" --version &>/dev/null \
        || die "El binario descargado no responde correctamente. Abortando."

    install_binary "$tmpfile"
    info "✓ ${BINARY} ${version} instalado en ${INSTALL_DIR}/${BINARY}"
    "${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || true
}

# ─── Aviso sobre dependencias opcionales ───────────────────────
check_optional_deps() {
    command -v mpv &>/dev/null \
        || warn "mpv no encontrado — instálalo para reproducción directa sin anuncios"
}

# ═══════════════════════════════════════════════════════════════
# Punto de entrada
# ═══════════════════════════════════════════════════════════════
main() {
    case "${1:-}" in
        --help|-h)
            usage
            ;;
        --release)
            install_from_release
            check_optional_deps
            ;;
        "")
            install_from_source
            check_optional_deps
            ;;
        *)
            die "Opción desconocida: '$1'. Usa --help para ver las opciones disponibles."
            ;;
    esac
}

main "$@"

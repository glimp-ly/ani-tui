#!/usr/bin/env bash
# install.sh - Instalador de ani-tui
# Uso:
#   ./install.sh            -> compilar e instalar desde el repo local
#   ./install.sh --release  -> descargar e instalar el binario del ultimo release de GitHub
#   ./install.sh --help     -> mostrar ayuda

set -euo pipefail

# --- Constantes ------------------------------------------------
BINARY="ani-tui"
REPO="glimp-ly/ani-tui"
INSTALL_DIR="${PREFIX:-/usr/local}/bin"
GITHUB_API="https://api.github.com/repos/${REPO}/releases/latest"
_TMPDIR=""  # scope global para que el trap EXIT siempre lo vea

_cleanup() { [ -n "$_TMPDIR" ] && rm -rf "$_TMPDIR"; }
trap '_cleanup' EXIT

# --- Colores (solo si el terminal los soporta) -----------------
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

info()  { printf "%s==>%s %s\n" "${C_GREEN}${C_BOLD}" "${C_RESET}" "$*"; }
warn()  { printf "%s[!]%s %s\n" "${C_YELLOW}" "${C_RESET}" "$*"; }
die()   { printf "%s[x]%s %s\n" "${C_RED}" "${C_RESET}" "$*" >&2; exit 1; }
step()  { printf "%s  ->%s %s\n" "${C_CYAN}" "${C_RESET}" "$*"; }

# --- Ayuda -----------------------------------------------------
usage() {
    cat <<EOF
${C_BOLD}ani-tui installer${C_RESET}

Uso: $0 [OPCION]

Opciones:
  (sin args)   Compilar e instalar desde el repo local usando Cargo
  --release    Descargar e instalar el binario del ultimo GitHub release
  --help       Mostrar esta ayuda

Destino de instalacion: ${INSTALL_DIR}
  Puede cambiarse con la variable PREFIX:  PREFIX=~/.local ./install.sh

Dependencias opcionales:
  mpv          Reproductor de video (recomendado para reproduccion sin anuncios)
  xdg-utils    Para abrir URLs en el navegador (fallback)
EOF
}

# --- Verificar comando disponible ------------------------------
need() {
    command -v "$1" &>/dev/null || die "Se requiere '$1' pero no esta instalado."
}

# --- Detectar arquitectura -------------------------------------
detect_arch() {
    local machine
    machine=$(uname -m)
    case "$machine" in
        x86_64)        echo "x86_64-unknown-linux-gnu" ;;
        aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
        *)             die "Arquitectura no soportada: $machine" ;;
    esac
}

# --- Determinar si se requiere sudo ----------------------------
needs_sudo() {
    [ ! -w "$INSTALL_DIR" ]
}

# --- Instalar el binario (con o sin sudo) ----------------------
install_binary() {
    local src="$1"
    local dest="${INSTALL_DIR}/${BINARY}"
    mkdir -p "$INSTALL_DIR" 2>/dev/null || true
    if needs_sudo; then
        step "Instalando en ${dest} (se requerira contrasena)..."
        sudo install -Dm755 "$src" "$dest"
    else
        install -Dm755 "$src" "$dest"
    fi
}

# ==============================================================
# MODO 1: Compilar desde el repo local
# ==============================================================
install_from_source() {
    info "Instalando ${BINARY} desde el codigo fuente..."
    need cargo
    [ -f Cargo.toml ] || die "Ejecuta el script desde la raiz del repositorio."
    grep -q "name = \"${BINARY}\"" Cargo.toml \
        || die "Este no parece ser el repositorio de ${BINARY}."
    step "Compilando en modo release (esto puede tardar unos minutos)..."
    cargo build --release --quiet
    local built="target/release/${BINARY}"
    [ -f "$built" ] || die "La compilacion fallo: no se encontro el binario."
    install_binary "$built"
    info "OK: ${BINARY} instalado en ${INSTALL_DIR}/${BINARY}"
    "${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || true
}

# ==============================================================
# MODO 2: Descargar el binario del ultimo GitHub release
# ==============================================================
install_from_release() {
    info "Instalando/actualizando ${BINARY} desde el ultimo GitHub release..."
    need curl

    step "Consultando el ultimo release..."
    local release_json
    release_json=$(curl -fsSL "$GITHUB_API") \
        || die "No se pudo conectar a la API de GitHub. Verifica tu conexion."

    # Extraer tag y nombre del release
    local tag_name release_name
    tag_name=$(printf '%s' "$release_json" | grep '"tag_name"' | head -1 \
               | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
    release_name=$(printf '%s' "$release_json" | grep '"name"' | head -1 \
                   | sed 's/.*"name": *"\([^"]*\)".*/\1/')
    [ -n "$tag_name" ] || die "No se pudo obtener informacion del release."

    # Version a mostrar: usar nombre del release si contiene un numero de version
    local display_version="$tag_name"
    printf '%s' "$release_name" | grep -qE '[0-9]+\.[0-9]+' \
        && display_version="$release_name"

    step "Ultimo release: ${display_version} (tag: ${tag_name})"

    # Comparar version solo si el tag es semver (ej. v0.2.0)
    local tag_semver="${tag_name#v}"
    if command -v "$BINARY" &>/dev/null \
       && printf '%s' "$tag_semver" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        local current
        current=$("$BINARY" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)
        if [ -n "$current" ] && [ "$current" = "$tag_semver" ]; then
            info "Ya tienes la version mas reciente: ${display_version}"
            exit 0
        fi
        [ -n "$current" ] && step "Actualizando ${current} -> ${tag_semver}..."
    fi

    local arch
    arch=$(detect_arch)

    # Obtener todas las URLs de assets del release
    local all_urls
    all_urls=$(printf '%s' "$release_json" \
        | grep '"browser_download_url"' \
        | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/')

    local download_url=""

    # Prioridad 1: binario + arquitectura + tarball
    #   ej: ani-tui-x86_64-unknown-linux-gnu.tar.gz
    download_url=$(printf '%s' "$all_urls" \
        | grep "${BINARY}-${arch}\.tar\.gz" | head -1 || true)

    # Prioridad 2: binario + arquitectura sin extension
    #   ej: ani-tui-x86_64-unknown-linux-gnu
    if [ -z "$download_url" ]; then
        download_url=$(printf '%s' "$all_urls" \
            | grep "${BINARY}-${arch}" \
            | grep -vE '\.(tar\.gz|zip|sha|sig)$' \
            | head -1 || true)
    fi

    # Prioridad 3: binario con nombre exacto del proyecto (sin arquitectura)
    #   ej: ani-tui  (formato actual del release)
    if [ -z "$download_url" ]; then
        download_url=$(printf '%s' "$all_urls" \
            | grep -E "/${BINARY}$" \
            | head -1 || true)
    fi

    # Prioridad 4: cualquier asset ejecutable (excluye checksums y firmas)
    if [ -z "$download_url" ]; then
        download_url=$(printf '%s' "$all_urls" \
            | grep -vE '\.(sha256|sha512|asc|sig|tar\.gz|zip)$' \
            | head -1 || true)
    fi

    [ -n "$download_url" ] \
        || die "No se encontro ningun binario en el release '${tag_name}'."

    step "Descargando: ${download_url##*/}"

    # Directorio temporal — variable global para que el trap EXIT lo alcance
    _TMPDIR=$(mktemp -d)

    local tmpfile="${_TMPDIR}/${BINARY}"
    curl -fsSL --progress-bar -o "${tmpfile}.download" "$download_url"

    # Detectar si es tarball o binario directo
    local mime
    mime=$(file -b "${tmpfile}.download" 2>/dev/null || true)

    if printf '%s' "$mime" | grep -qi 'gzip\|tar'; then
        step "Extrayendo tarball..."
        tar -xzf "${tmpfile}.download" -C "$_TMPDIR"
        local extracted
        extracted=$(find "$_TMPDIR" -maxdepth 3 -type f -name "$BINARY" | head -1)
        [ -n "$extracted" ] || die "No se encontro '${BINARY}' dentro del tarball."
        mv "$extracted" "$tmpfile"
    else
        mv "${tmpfile}.download" "$tmpfile"
    fi

    chmod +x "$tmpfile"

    # Verificar que el binario funciona antes de instalarlo
    "$tmpfile" --version &>/dev/null \
        || die "El binario descargado no responde correctamente. Abortando."

    install_binary "$tmpfile"
    # Limpiar explicitamente (el trap EXIT es red de seguridad adicional)
    _cleanup
    info "OK: ${BINARY} instalado en ${INSTALL_DIR}/${BINARY}"
    "${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || true
}

# --- Aviso sobre dependencias opcionales -----------------------
check_optional_deps() {
    command -v mpv &>/dev/null \
        || warn "mpv no encontrado -- instalalo para reproduccion directa sin anuncios"
}

# ==============================================================
# Punto de entrada
# ==============================================================
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
            die "Opcion desconocida: '$1'. Usa --help para ver las opciones."
            ;;
    esac
}

main "$@"

#!/bin/sh
# MemX installer for macOS and Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/memxlab/memx/main/install.sh | sh
set -e

REPO="${MEMX_REPO:-memxlab/memx}"
VERSION="${MEMX_VERSION:-latest}"
DOWNLOAD_BASE_URL="${MEMX_DOWNLOAD_BASE_URL:-}"
BINARY_NAME="memx"
MEMX_HOME="${MEMX_HOME:-$HOME/.memx}"
FALLBACK_INSTALL_DIR="$MEMX_HOME/bin"

if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    BOLD='\033[1m'
    NC='\033[0m'
else
    RED='' GREEN='' YELLOW='' BOLD='' NC=''
fi

info()    { printf "${GREEN}==${NC} %s\n" "$1"; }
success() { printf "${GREEN}✓${NC}  %s\n" "$1"; }
warn()    { printf "${YELLOW}!${NC}  %s\n" "$1" >&2; }
error()   { printf "${RED}✗${NC}  %s\n" "$1" >&2; exit 1; }
step()    { printf "${BOLD}→${NC}  %s\n" "$1"; }

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        error "Required command not found: $1"
    fi
}

detect_os() {
    case "$(uname -s)" in
        Linux)  echo "linux"  ;;
        Darwin) echo "darwin" ;;
        *)      error "Unsupported OS: $(uname -s)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64)        echo "x86_64"  ;;
        aarch64|arm64) echo "aarch64" ;;
        *)             error "Unsupported architecture: $(uname -m)" ;;
    esac
}

path_contains() {
    case ":$PATH:" in
        *:"$1":*) return 0 ;;
        *)        return 1 ;;
    esac
}

find_writable_ancestor() {
    DIR="$1"
    while [ ! -d "$DIR" ]; do
        NEXT=$(dirname "$DIR")
        if [ "$NEXT" = "$DIR" ]; then
            return 1
        fi
        DIR="$NEXT"
    done

    [ -w "$DIR" ]
}

choose_install_dir() {
    if [ -n "$MEMX_INSTALL_DIR" ]; then
        printf "%s\n" "$MEMX_INSTALL_DIR"
        return
    fi

    for candidate in "$HOME/.local/bin" "$HOME/bin" "/opt/homebrew/bin" "/usr/local/bin"; do
        if path_contains "$candidate" && find_writable_ancestor "$candidate"; then
            printf "%s\n" "$candidate"
            return
        fi
    done

    printf "%s\n" "$FALLBACK_INSTALL_DIR"
}

resolve_asset_name() {
    OS="$1"
    ARCH="$2"

    case "$OS/$ARCH" in
        darwin/x86_64|darwin/aarch64)
            echo "memx-darwin-universal.tar.gz"
            ;;
        linux/x86_64)
            echo "memx-linux-x86_64.tar.gz"
            ;;
        linux/aarch64)
            error "Linux aarch64 artifacts are not published yet."
            ;;
        *)
            error "Unsupported target: $OS/$ARCH"
            ;;
    esac
}

resolve_download_url() {
    ASSET_NAME="$1"

    if [ -n "$DOWNLOAD_BASE_URL" ]; then
        printf "%s/%s\n" "${DOWNLOAD_BASE_URL%/}" "$ASSET_NAME"
        return
    fi

    if [ "$VERSION" = "latest" ]; then
        printf "https://github.com/%s/releases/latest/download/%s\n" "$REPO" "$ASSET_NAME"
        return
    fi

    printf "https://github.com/%s/releases/download/%s/%s\n" "$REPO" "$VERSION" "$ASSET_NAME"
}

download_archive() {
    URL="$1"
    DEST="$2"

    step "Downloading $URL"

    if command -v curl > /dev/null 2>&1; then
        curl -fsSL --retry 3 --retry-delay 2 \
            --connect-timeout 15 --max-time 300 \
            "$URL" -o "$DEST" \
            || error "Download failed"
        return
    fi

    if command -v wget > /dev/null 2>&1; then
        wget -q --tries=3 --timeout=300 "$URL" -O "$DEST" \
            || error "Download failed"
        return
    fi

    error "Neither curl nor wget is available"
}

extract_binary() {
    ARCHIVE="$1"
    EXTRACT_DIR="$2"

    mkdir -p "$EXTRACT_DIR"
    tar -xzf "$ARCHIVE" -C "$EXTRACT_DIR" || error "Failed to extract archive"

    FOUND=$(find "$EXTRACT_DIR" -type f -name "$BINARY_NAME" | head -n 1)
    if [ -z "$FOUND" ]; then
        error "Could not find $BINARY_NAME inside archive"
    fi

    printf "%s\n" "$FOUND"
}

install_binary() {
    SOURCE="$1"
    TARGET_DIR="$2"
    TARGET="$TARGET_DIR/$BINARY_NAME"

    mkdir -p "$TARGET_DIR"
    chmod +x "$SOURCE"

    if [ -f "$TARGET" ]; then
        warn "Found existing $TARGET — backing up to ${TARGET}.bak"
        cp "$TARGET" "${TARGET}.bak"
    fi

    cp "$SOURCE" "$TARGET"
    chmod +x "$TARGET"

    printf "%s\n" "$TARGET"
}

verify_install() {
    TARGET="$1"
    "$TARGET" --help > /dev/null 2>&1 || error "Installed binary failed verification"
    success "Binary verified OK"
}

run_setup() {
    TARGET="$1"

    if [ "${MEMX_INSTALL_SKIP_SETUP:-0}" = "1" ]; then
        warn "Skipping memx setup because MEMX_INSTALL_SKIP_SETUP=1"
        return
    fi

    printf "\n"
    if [ -t 0 ] && [ -t 1 ]; then
        step "Launching memx setup"
        "$TARGET" setup
        return
    fi

    if [ -r /dev/tty ] && [ -w /dev/tty ]; then
        step "Launching memx setup"
        "$TARGET" setup < /dev/tty > /dev/tty 2> /dev/tty
        return
    fi

    warn "No interactive terminal detected. Run this next:"
    printf "  %s setup\n" "$TARGET"
}

print_path_hint() {
    INSTALL_DIR="$1"

    if path_contains "$INSTALL_DIR"; then
        return
    fi

    printf "\n"
    warn "The install directory is not on your PATH"
    printf "Add this line to your shell profile:\n"
    printf "  export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR"
}

print_next_steps() {
    TARGET="$1"
    INSTALL_DIR=$(dirname "$TARGET")

    printf "\n"
    printf "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"
    printf "${GREEN}  MemX installed successfully!${NC}\n"
    printf "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"
    printf "\n"
    printf "Binary path:\n"
    printf "  %s\n" "$TARGET"
    print_path_hint "$INSTALL_DIR"
    printf "\n"
    printf "Next steps:\n"
    printf "  1. Run setup if you skipped it:\n"
    printf "     %s setup\n" "$TARGET"
    printf "  2. Validate config:\n"
    printf "     %s doctor\n" "$TARGET"
    printf "  3. Start the server:\n"
    printf "     %s serve\n" "$TARGET"
    printf "\n"
}

main() {
    printf "\n"
    info "MemX Installer"
    printf "\n"

    need_cmd uname
    need_cmd mktemp
    need_cmd tar
    need_cmd find

    OS=$(detect_os)
    ARCH=$(detect_arch)
    INSTALL_DIR=$(choose_install_dir)
    ASSET_NAME=$(resolve_asset_name "$OS" "$ARCH")
    DOWNLOAD_URL=$(resolve_download_url "$ASSET_NAME")

    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

    ARCHIVE_PATH="$TMP_DIR/$ASSET_NAME"
    EXTRACT_DIR="$TMP_DIR/extracted"

    download_archive "$DOWNLOAD_URL" "$ARCHIVE_PATH"
    BINARY_PATH=$(extract_binary "$ARCHIVE_PATH" "$EXTRACT_DIR")
    TARGET_PATH=$(install_binary "$BINARY_PATH" "$INSTALL_DIR")

    success "Installed to $TARGET_PATH"
    verify_install "$TARGET_PATH"
    run_setup "$TARGET_PATH"
    print_next_steps "$TARGET_PATH"
}

main "$@"

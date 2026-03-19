#!/bin/sh
# MemX installer for macOS and Linux
# Install:
#   curl -fsSL https://raw.githubusercontent.com/memxlab/memx/main/install.sh | sh
# Uninstall:
#   curl -fsSL https://raw.githubusercontent.com/memxlab/memx/main/install.sh | sh -s -- uninstall
set -e

REPO="${MEMX_REPO:-memxlab/memx}"
VERSION="${MEMX_VERSION:-latest}"
DOWNLOAD_BASE_URL="${MEMX_DOWNLOAD_BASE_URL:-}"
BINARY_NAME="memx"
MEMX_HOME="${MEMX_HOME:-}"
ACTION="${MEMX_ACTION:-install}"
ASSUME_YES=0
TARGET_USER=""
TARGET_HOME=""
TARGET_GROUP=""

if [ "${MEMX_UNINSTALL:-0}" = "1" ]; then
    ACTION="uninstall"
fi

if [ "${MEMX_YES:-0}" = "1" ] || [ "${MEMX_UNINSTALL_YES:-0}" = "1" ]; then
    ASSUME_YES=1
fi

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

usage() {
    cat <<EOF
Usage:
  install.sh [install] [--yes]
  install.sh uninstall [--yes]

Environment variables:
  MEMX_VERSION               Release tag to install (default: latest)
  MEMX_REPO                  GitHub repo in owner/name format
  MEMX_DOWNLOAD_BASE_URL     Override release asset base URL
  MEMX_HOME                  Override MemX data directory
  MEMX_TARGET_USER           Override the runtime user used for setup/service
  MEMX_TARGET_HOME           Override the runtime user's home directory
  MEMX_INSTALL_DIR           Override install directory
  MEMX_INSTALL_SKIP_SETUP    Skip running 'memx setup' after install when set to 1
  MEMX_INSTALL_SKIP_SERVICE  Skip prompting to install/start the background service
  MEMX_INSTALL_START_SERVICE Set to 1 to auto-install/start the background service without prompting
  MEMX_UNINSTALL             Set to 1 to run uninstall mode
  MEMX_YES                   Set to 1 to skip confirmation prompts
EOF
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        error "Required command not found: $1"
    fi
}

parse_args() {
    for arg in "$@"; do
        case "$arg" in
            install|uninstall)
                ACTION="$arg"
                ;;
            --yes|-y)
                ASSUME_YES=1
                ;;
            --help|-h)
                usage
                exit 0
                ;;
            *)
                error "Unknown argument: $arg"
                ;;
        esac
    done
}

current_user() {
    id -un
}

is_root_user() {
    [ "$(id -u)" -eq 0 ]
}

resolve_user_home() {
    USER_NAME="$1"

    if [ "$USER_NAME" = "$(current_user)" ] && [ -n "$HOME" ]; then
        printf "%s\n" "$HOME"
        return 0
    fi

    HOME_PATH=$(eval "printf '%s' ~$USER_NAME" 2> /dev/null || true)
    case "$HOME_PATH" in
        "~$USER_NAME"|~) ;;
        *)
            if [ -n "$HOME_PATH" ]; then
                printf "%s\n" "$HOME_PATH"
                return 0
            fi
            ;;
    esac

    if command -v getent > /dev/null 2>&1; then
        HOME_PATH=$(getent passwd "$USER_NAME" | cut -d: -f6)
        if [ -n "$HOME_PATH" ]; then
            printf "%s\n" "$HOME_PATH"
            return 0
        fi
    fi

    if command -v dscl > /dev/null 2>&1; then
        HOME_PATH=$(dscl . -read "/Users/$USER_NAME" NFSHomeDirectory 2> /dev/null | awk '{print $2}')
        if [ -n "$HOME_PATH" ]; then
            printf "%s\n" "$HOME_PATH"
            return 0
        fi
    fi

    return 1
}

resolve_target_identity() {
    if [ -n "${MEMX_TARGET_USER:-}" ]; then
        TARGET_USER="$MEMX_TARGET_USER"
    elif [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
        TARGET_USER="$SUDO_USER"
    else
        TARGET_USER=$(current_user)
    fi

    if [ -n "${MEMX_TARGET_HOME:-}" ]; then
        TARGET_HOME="$MEMX_TARGET_HOME"
    else
        TARGET_HOME=$(resolve_user_home "$TARGET_USER") || error "Cannot determine home directory for $TARGET_USER"
    fi

    TARGET_GROUP=$(id -gn "$TARGET_USER" 2> /dev/null || true)

    if [ -z "$MEMX_HOME" ]; then
        MEMX_HOME="$TARGET_HOME/.memx"
    fi
}

run_as_target_user() {
    if [ "$(current_user)" = "$TARGET_USER" ]; then
        HOME="$TARGET_HOME" USER="$TARGET_USER" LOGNAME="$TARGET_USER" MEMX_HOME="$MEMX_HOME" "$@"
        return
    fi

    if command -v sudo > /dev/null 2>&1; then
        sudo -H -u "$TARGET_USER" env \
            HOME="$TARGET_HOME" \
            USER="$TARGET_USER" \
            LOGNAME="$TARGET_USER" \
            MEMX_HOME="$MEMX_HOME" \
            "$@"
        return
    fi

    error "Cannot switch to $TARGET_USER because sudo is not available"
}

chown_target_path() {
    TARGET_PATH="$1"

    if ! is_root_user || [ "$TARGET_USER" = "root" ] || [ -z "$TARGET_GROUP" ] || [ ! -e "$TARGET_PATH" ]; then
        return
    fi

    chown "$TARGET_USER:$TARGET_GROUP" "$TARGET_PATH" \
        || error "Failed to set ownership on $TARGET_PATH"
}

chown_target_tree() {
    TARGET_PATH="$1"

    if ! is_root_user || [ "$TARGET_USER" = "root" ] || [ -z "$TARGET_GROUP" ] || [ ! -e "$TARGET_PATH" ]; then
        return
    fi

    chown -R "$TARGET_USER:$TARGET_GROUP" "$TARGET_PATH" \
        || error "Failed to set ownership on $TARGET_PATH"
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
    if [ -n "${MEMX_INSTALL_DIR:-}" ]; then
        printf "%s\n" "$MEMX_INSTALL_DIR"
        return
    fi

    for candidate in "$TARGET_HOME/.local/bin" "$TARGET_HOME/bin"; do
        if find_writable_ancestor "$candidate"; then
            printf "%s\n" "$candidate"
            return
        fi
    done

    printf "%s\n" "$MEMX_HOME/bin"
}

find_installed_binary() {
    if [ -n "${MEMX_INSTALL_DIR:-}" ] && [ -f "$MEMX_INSTALL_DIR/$BINARY_NAME" ]; then
        printf "%s\n" "$MEMX_INSTALL_DIR/$BINARY_NAME"
        return 0
    fi

    for candidate in \
        "$TARGET_HOME/.local/bin/$BINARY_NAME" \
        "$TARGET_HOME/bin/$BINARY_NAME" \
        "$MEMX_HOME/bin/$BINARY_NAME"
    do
        if [ -f "$candidate" ]; then
            printf "%s\n" "$candidate"
            return 0
        fi
    done

    if command -v "$BINARY_NAME" > /dev/null 2>&1; then
        CMD_PATH=$(command -v "$BINARY_NAME")
        case "$CMD_PATH" in
            /*)
                if [ -f "$CMD_PATH" ]; then
                    printf "%s\n" "$CMD_PATH"
                    return 0
                fi
                ;;
        esac
    fi

    return 1
}

prompt_yes_no_tty() {
    LABEL="$1"
    DEFAULT="$2"

    if [ ! -r /dev/tty ] || [ ! -w /dev/tty ]; then
        error "No interactive terminal detected. Re-run with --yes to continue."
    fi

    while :; do
        if [ "$DEFAULT" = "yes" ]; then
            SUFFIX="[Y/n]"
        else
            SUFFIX="[y/N]"
        fi

        printf "%s %s " "$LABEL" "$SUFFIX" > /dev/tty
        IFS= read -r ANSWER < /dev/tty || error "Failed to read confirmation from terminal"

        if [ -z "$ANSWER" ]; then
            [ "$DEFAULT" = "yes" ] && return 0 || return 1
        fi

        case "$(printf '%s' "$ANSWER" | tr '[:upper:]' '[:lower:]')" in
            y|yes) return 0 ;;
            n|no)  return 1 ;;
            *)     printf "Please answer y or n.\n" > /dev/tty ;;
        esac
    done
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
    chown_target_tree "$TARGET_DIR"
    chmod +x "$SOURCE"

    if [ -f "$TARGET" ]; then
        warn "Found existing $TARGET — backing up to ${TARGET}.bak"
        cp "$TARGET" "${TARGET}.bak"
        chown_target_path "${TARGET}.bak"
    fi

    cp "$SOURCE" "$TARGET"
    chmod +x "$TARGET"
    chown_target_path "$TARGET"

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
        step "Launching memx setup for $TARGET_USER"
        run_as_target_user "$TARGET" setup
        return
    fi

    if [ -r /dev/tty ] && [ -w /dev/tty ]; then
        step "Launching memx setup for $TARGET_USER"
        run_as_target_user "$TARGET" setup < /dev/tty > /dev/tty 2> /dev/tty
        return
    fi

    warn "No interactive terminal detected. Run this next:"
    printf "  HOME=\"%s\" MEMX_HOME=\"%s\" %s setup\n" "$TARGET_HOME" "$MEMX_HOME" "$TARGET"
}

run_background_service() {
    TARGET="$1"

    if [ "${MEMX_INSTALL_SKIP_SETUP:-0}" = "1" ] || [ "${MEMX_INSTALL_SKIP_SERVICE:-0}" = "1" ]; then
        return
    fi

    SHOULD_START=0
    if [ "${MEMX_INSTALL_START_SERVICE:-0}" = "1" ]; then
        SHOULD_START=1
    elif [ -r /dev/tty ] && [ -w /dev/tty ]; then
        if prompt_yes_no_tty "Install and start MemX as a background service now?" "yes"; then
            SHOULD_START=1
        fi
    else
        warn "No interactive terminal detected. Run this next to start MemX in the background:"
        printf "  HOME=\"%s\" MEMX_HOME=\"%s\" %s service install\n" "$TARGET_HOME" "$MEMX_HOME" "$TARGET"
    fi

    if [ "$SHOULD_START" != "1" ]; then
        return
    fi

    step "Installing and starting the MemX background service for $TARGET_USER"
    run_as_target_user "$TARGET" service install || error "Failed to install/start the background service"
    success "MemX background service is running"
}

print_path_hint() {
    INSTALL_DIR="$1"

    printf "\n"
    warn "If '$INSTALL_DIR' is not on your PATH, add this line to your shell profile:"
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
    printf "Runtime user:\n"
    printf "  %s\n" "$TARGET_USER"
    printf "Data directory:\n"
    printf "  %s\n" "$MEMX_HOME"
    printf "Binary path:\n"
    printf "  %s\n" "$TARGET"
    print_path_hint "$INSTALL_DIR"
    printf "\n"
    printf "Next steps:\n"
    printf "  1. Validate config:\n"
    printf "     HOME=\"%s\" MEMX_HOME=\"%s\" %s doctor\n" "$TARGET_HOME" "$MEMX_HOME" "$TARGET"
    printf "  2. Check service status:\n"
    printf "     HOME=\"%s\" MEMX_HOME=\"%s\" %s service status\n" "$TARGET_HOME" "$MEMX_HOME" "$TARGET"
    printf "  3. Remove the background service:\n"
    printf "     HOME=\"%s\" MEMX_HOME=\"%s\" %s service remove\n" "$TARGET_HOME" "$MEMX_HOME" "$TARGET"
    printf "  4. Uninstall MemX and delete local data:\n"
    printf "     HOME=\"%s\" MEMX_HOME=\"%s\" %s uninstall\n" "$TARGET_HOME" "$MEMX_HOME" "$TARGET"
    printf "\n"
}

run_install() {
    printf "\n"
    info "MemX Installer"
    printf "\n"

    need_cmd uname
    need_cmd mktemp
    need_cmd tar
    need_cmd find
    need_cmd id

    resolve_target_identity

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
    run_background_service "$TARGET_PATH"
    print_next_steps "$TARGET_PATH"
}

remove_background_service() {
    TARGET_PATH="$1"

    if [ -z "$TARGET_PATH" ] || [ ! -x "$TARGET_PATH" ]; then
        return
    fi

    if run_as_target_user "$TARGET_PATH" service remove > /dev/null 2>&1; then
        success "Removed MemX background service"
    fi
}

run_uninstall() {
    resolve_target_identity
    TARGET_PATH=$(find_installed_binary || true)

    printf "\n"
    info "MemX Uninstall"
    printf "\n"
    warn "This will permanently delete your local MemX data."
    printf "Paths to be removed:\n"
    printf "  - %s\n" "$MEMX_HOME"
    printf "    This includes config.toml, memory.db, and any backups under ~/.memx.\n"

    if [ -n "$TARGET_PATH" ]; then
        printf "  - %s\n" "$TARGET_PATH"
        if [ -f "${TARGET_PATH}.bak" ]; then
            printf "  - %s.bak\n" "$TARGET_PATH"
        fi
    else
        printf "  - %s binary in common install paths (not found)\n" "$BINARY_NAME"
    fi

    printf "\n"
    warn "Your local MemX data will be lost."

    if [ "$ASSUME_YES" != "1" ] && ! prompt_yes_no_tty "Continue uninstall?" "no"; then
        printf "Uninstall canceled.\n"
        return 0
    fi

    remove_background_service "$TARGET_PATH"

    if [ -n "$TARGET_PATH" ] && [ -e "$TARGET_PATH" ]; then
        rm -f "$TARGET_PATH" || error "Failed to remove $TARGET_PATH"
        success "Removed $TARGET_PATH"
    else
        warn "Skipped binary removal (not found)"
    fi

    if [ -n "$TARGET_PATH" ] && [ -e "${TARGET_PATH}.bak" ]; then
        rm -f "${TARGET_PATH}.bak" || error "Failed to remove ${TARGET_PATH}.bak"
        success "Removed ${TARGET_PATH}.bak"
    fi

    if [ -d "$MEMX_HOME" ]; then
        rm -rf "$MEMX_HOME" || error "Failed to remove $MEMX_HOME"
        success "Removed $MEMX_HOME"
    else
        warn "Skipped $MEMX_HOME (not found)"
    fi

    printf "\n"
    success "MemX uninstall complete"
}

main() {
    parse_args "$@"

    case "$ACTION" in
        install)
            run_install
            ;;
        uninstall)
            run_uninstall
            ;;
        *)
            error "Unsupported action: $ACTION"
            ;;
    esac
}

main "$@"

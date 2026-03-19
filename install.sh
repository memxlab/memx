#!/bin/sh
# MemX Installer
# Usage: curl -fsSL https://memx.me/install.sh | sudo sh
set -e

# ── Configuration ─────────────────────────────────────────────────────────────

BASE_URL="https://diandi-app.oss-cn-hangzhou.aliyuncs.com/other"
BINARY_NAME="memx"
INSTALL_DIR="/usr/local/bin"
DEFAULT_EMBEDDING_BASE_URL="https://api.deepinfra.com/v1/openai"
DEFAULT_EMBEDDING_MODEL="Qwen/Qwen3-Embedding-0.6B"
DEFAULT_EMBEDDING_DIMENSION="1024"

INTERACTIVE=0
CONFIG_CREATED=0
CONFIG_SKIPPED=0
CONFIG_OWNER=""
CONFIG_HOME=""
CONFIG_DIR=""
CONFIG_PATH=""
PROMPT_RESULT=""

# ── Colors (only when stdout is a terminal) ───────────────────────────────────

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

# ── Prerequisite checks ───────────────────────────────────────────────────────

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        error "Required command not found: $1. Please install it and try again."
    fi
}

check_root() {
    if [ "$(id -u)" -ne 0 ]; then
        error "This installer requires root. Please run with sudo:\n  curl -fsSL https://memx.me/install.sh | sudo sh"
    fi
}

setup_prompt_io() {
    if [ -t 0 ] && [ -t 1 ]; then
        INTERACTIVE=1
        exec 3<&0 4>&1
        return
    fi

    if [ -r /dev/tty ] && [ -w /dev/tty ]; then
        INTERACTIVE=1
        exec 3</dev/tty 4>/dev/tty
        return
    fi

    INTERACTIVE=0
}

resolve_user_home() {
    USER_NAME="$1"
    USER_HOME=""

    if [ -z "$USER_NAME" ]; then
        printf "%s\n" "$HOME"
        return
    fi

    if command -v getent > /dev/null 2>&1; then
        USER_HOME=$(getent passwd "$USER_NAME" | awk -F: 'NR == 1 { print $6 }')
    fi

    if [ -z "$USER_HOME" ] && command -v dscl > /dev/null 2>&1; then
        USER_HOME=$(dscl . -read "/Users/$USER_NAME" NFSHomeDirectory 2> /dev/null | awk 'NR == 1 { print $2 }')
    fi

    if [ -z "$USER_HOME" ] && [ "$(id -un 2> /dev/null)" = "$USER_NAME" ]; then
        USER_HOME="$HOME"
    fi

    if [ -z "$USER_HOME" ]; then
        error "Cannot determine home directory for user: $USER_NAME"
    fi

    printf "%s\n" "$USER_HOME"
}

detect_config_target() {
    if [ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "root" ]; then
        CONFIG_OWNER="$SUDO_USER"
    else
        CONFIG_OWNER=$(id -un)
    fi

    CONFIG_HOME=$(resolve_user_home "$CONFIG_OWNER")
    CONFIG_DIR="$CONFIG_HOME/.memx"
    CONFIG_PATH="$CONFIG_DIR/config.toml"
}

prompt_text() {
    LABEL="$1"
    DEFAULT_VALUE="$2"

    if [ -n "$DEFAULT_VALUE" ]; then
        printf "%s [%s]: " "$LABEL" "$DEFAULT_VALUE" >&4
    else
        printf "%s: " "$LABEL" >&4
    fi

    IFS= read -r PROMPT_RESULT <&3 || PROMPT_RESULT=""

    if [ -z "$PROMPT_RESULT" ]; then
        PROMPT_RESULT="$DEFAULT_VALUE"
    fi
}

prompt_secret() {
    LABEL="$1"
    DEFAULT_VALUE="$2"
    INPUT_VALUE=""

    while :; do
        if [ -n "$DEFAULT_VALUE" ]; then
            printf "%s [press Enter to keep current value]: " "$LABEL" >&4
        else
            printf "%s: " "$LABEL" >&4
        fi

        if command -v stty > /dev/null 2>&1; then
            STTY_STATE=$(stty -g <&3 2> /dev/null || true)
            stty -echo <&3 2> /dev/null || true
            IFS= read -r INPUT_VALUE <&3 || INPUT_VALUE=""
            if [ -n "$STTY_STATE" ]; then
                stty "$STTY_STATE" <&3 2> /dev/null || true
            else
                stty echo <&3 2> /dev/null || true
            fi
            printf "\n" >&4
        else
            IFS= read -r INPUT_VALUE <&3 || INPUT_VALUE=""
        fi

        if [ -z "$INPUT_VALUE" ] && [ -n "$DEFAULT_VALUE" ]; then
            INPUT_VALUE="$DEFAULT_VALUE"
        fi

        if [ -n "$INPUT_VALUE" ]; then
            PROMPT_RESULT="$INPUT_VALUE"
            return
        fi

        printf "API key cannot be empty.\n" >&4
    done
}

prompt_yes_no() {
    LABEL="$1"
    DEFAULT_ANSWER="$2"

    while :; do
        case "$DEFAULT_ANSWER" in
            Y|y) SUFFIX="[Y/n]" ;;
            N|n) SUFFIX="[y/N]" ;;
            *)   SUFFIX="[y/n]" ;;
        esac

        printf "%s %s: " "$LABEL" "$SUFFIX" >&4
        IFS= read -r ANSWER <&3 || ANSWER=""

        if [ -z "$ANSWER" ]; then
            ANSWER="$DEFAULT_ANSWER"
        fi

        case "$ANSWER" in
            Y|y|yes|YES|Yes) return 0 ;;
            N|n|no|NO|No)    return 1 ;;
        esac

        printf "Please answer y or n.\n" >&4
    done
}

prompt_dimension() {
    DEFAULT_VALUE="$1"

    while :; do
        prompt_text "Embedding dimension" "$DEFAULT_VALUE"

        case "$PROMPT_RESULT" in
            ''|*[!0-9]*)
                printf "Please enter a positive integer.\n" >&4
                ;;
            0)
                printf "Please enter a positive integer.\n" >&4
                ;;
            *)
                return
                ;;
        esac
    done
}

read_embedding_value() {
    FILE_PATH="$1"
    KEY_NAME="$2"

    if [ ! -f "$FILE_PATH" ]; then
        return
    fi

    awk -v key="$KEY_NAME" '
        BEGIN { in_embedding = 0 }
        /^[[:space:]]*\[embedding\][[:space:]]*$/ {
            in_embedding = 1
            next
        }
        /^[[:space:]]*\[[^]]+\][[:space:]]*$/ {
            if (in_embedding) {
                exit
            }
            next
        }
        in_embedding {
            line = $0
            sub(/^[[:space:]]+/, "", line)
            if (line ~ "^" key "[[:space:]]*=") {
                sub("^[^=]*=[[:space:]]*", "", line)
                sub(/[[:space:]]*(#.*)?$/, "", line)
                if (line ~ /^".*"$/) {
                    sub(/^"/, "", line)
                    sub(/"$/, "", line)
                }
                print line
                exit
            }
        }
    ' "$FILE_PATH"
}

strip_embedding_section() {
    FILE_PATH="$1"

    awk '
        BEGIN { in_embedding = 0 }
        /^[[:space:]]*\[embedding\][[:space:]]*$/ {
            in_embedding = 1
            next
        }
        /^[[:space:]]*\[[^]]+\][[:space:]]*$/ {
            if (in_embedding) {
                in_embedding = 0
            }
        }
        !in_embedding { print }
    ' "$FILE_PATH"
}

toml_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

write_config_file() {
    API_KEY_ESCAPED=$(toml_escape "$EMBEDDING_API_KEY")
    BASE_URL_ESCAPED=$(toml_escape "$EMBEDDING_BASE_URL")
    MODEL_ESCAPED=$(toml_escape "$EMBEDDING_MODEL")
    DB_PATH_ESCAPED=$(toml_escape "$CONFIG_DIR/memory.db")

    TMP_CONFIG="$TMP_DIR/config.toml"
    PRESERVED_CONFIG="$TMP_DIR/config.preserved"

    if [ -f "$CONFIG_PATH" ]; then
        strip_embedding_section "$CONFIG_PATH" > "$PRESERVED_CONFIG"
    else
        : > "$PRESERVED_CONFIG"
    fi

    {
        printf "# MemX Configuration\n"
        printf "# Config:   %s\n" "$CONFIG_PATH"
        printf "# Database: %s\n\n" "$CONFIG_DIR/memory.db"
        printf "[embedding]\n"
        printf "api_key = \"%s\"\n" "$API_KEY_ESCAPED"
        printf "base_url = \"%s\"\n" "$BASE_URL_ESCAPED"
        printf "model = \"%s\"\n" "$MODEL_ESCAPED"
        printf "dimension = %s\n" "$EMBEDDING_DIMENSION"

        if ! grep -Eq '^[[:space:]]*\[server\][[:space:]]*$' "$PRESERVED_CONFIG"; then
            printf "\n[server]\n"
            printf "host = \"127.0.0.1\"\n"
            printf "port = 7878\n"
            printf "\n# Uncomment to use a custom database path:\n"
            printf "# [database]\n"
            printf "# path = \"%s\"\n" "$DB_PATH_ESCAPED"
        fi

        if [ -s "$PRESERVED_CONFIG" ]; then
            printf "\n"
            cat "$PRESERVED_CONFIG"
        fi
    } > "$TMP_CONFIG"

    mkdir -p "$CONFIG_DIR"
    chmod 700 "$CONFIG_DIR" 2> /dev/null || true

    if [ -f "$CONFIG_PATH" ]; then
        cp "$CONFIG_PATH" "${CONFIG_PATH}.bak"
        chown "$CONFIG_OWNER" "${CONFIG_PATH}.bak" 2> /dev/null || true
        success "Backed up existing config to ${CONFIG_PATH}.bak"
    fi

    mv "$TMP_CONFIG" "$CONFIG_PATH"
    chmod 600 "$CONFIG_PATH" 2> /dev/null || true
    chown "$CONFIG_OWNER" "$CONFIG_DIR" "$CONFIG_PATH" 2> /dev/null || true
}

configure_memx() {
    if [ "$INTERACTIVE" -ne 1 ]; then
        warn "No interactive terminal detected. Skipping config setup."
        CONFIG_SKIPPED=1
        return
    fi

    printf "\n" >&4
    printf "MemX can create or update the config now:\n" >&4
    printf "  %s\n" "$CONFIG_PATH" >&4

    if [ -f "$CONFIG_PATH" ]; then
        printf "Existing config will be backed up to:\n" >&4
        printf "  %s.bak\n" "$CONFIG_PATH" >&4
    fi

    printf "\n" >&4
    printf "Press Enter to accept the default value shown in brackets.\n" >&4
    printf "Make sure the dimension matches the embedding model.\n" >&4
    printf "\n" >&4

    if ! prompt_yes_no "Set up embedding config now?" "Y"; then
        CONFIG_SKIPPED=1
        return
    fi

    EXISTING_MODEL=$(read_embedding_value "$CONFIG_PATH" "model")
    EXISTING_BASE_URL=$(read_embedding_value "$CONFIG_PATH" "base_url")
    EXISTING_API_KEY=$(read_embedding_value "$CONFIG_PATH" "api_key")
    EXISTING_DIMENSION=$(read_embedding_value "$CONFIG_PATH" "dimension")

    if [ -z "$EXISTING_MODEL" ]; then
        EXISTING_MODEL="$DEFAULT_EMBEDDING_MODEL"
    fi
    if [ -z "$EXISTING_BASE_URL" ]; then
        EXISTING_BASE_URL="$DEFAULT_EMBEDDING_BASE_URL"
    fi
    if [ -z "$EXISTING_DIMENSION" ]; then
        EXISTING_DIMENSION="$DEFAULT_EMBEDDING_DIMENSION"
    fi

    prompt_text "Embedding model" "$EXISTING_MODEL"
    EMBEDDING_MODEL="$PROMPT_RESULT"

    prompt_text "Embedding base URL" "$EXISTING_BASE_URL"
    EMBEDDING_BASE_URL="$PROMPT_RESULT"

    prompt_secret "Embedding API key" "$EXISTING_API_KEY"
    EMBEDDING_API_KEY="$PROMPT_RESULT"

    prompt_dimension "$EXISTING_DIMENSION"
    EMBEDDING_DIMENSION="$PROMPT_RESULT"

    write_config_file
    success "Config saved to $CONFIG_PATH"
    CONFIG_CREATED=1
}

# ── Platform detection ────────────────────────────────────────────────────────

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

# ── Download ──────────────────────────────────────────────────────────────────

download_binary() {
    OS="$1"
    ARCH="$2"

    # Current release provides a single universal binary.
    # Future releases will use: ${BASE_URL}/memx-${OS}-${ARCH}
    DOWNLOAD_URL="${BASE_URL}/memx"

    step "Downloading memx ($OS/$ARCH)"
    step "  from $DOWNLOAD_URL"

    TMP_FILE="$TMP_DIR/$BINARY_NAME"

    if command -v curl > /dev/null 2>&1; then
        curl -fsSL --retry 3 --retry-delay 2 \
            --connect-timeout 15 --max-time 120 \
            "$DOWNLOAD_URL" -o "$TMP_FILE" \
            || error "Download failed. Check your network connection."
    elif command -v wget > /dev/null 2>&1; then
        wget -q --tries=3 --timeout=120 \
            "$DOWNLOAD_URL" -O "$TMP_FILE" \
            || error "Download failed. Check your network connection."
    else
        error "Neither curl nor wget is available. Please install one and retry."
    fi

    if [ ! -s "$TMP_FILE" ]; then
        error "Downloaded file is empty. The release artifact may not exist yet."
    fi
}

# ── Install ───────────────────────────────────────────────────────────────────

install_binary() {
    TMP_FILE="$TMP_DIR/$BINARY_NAME"
    TARGET="$INSTALL_DIR/$BINARY_NAME"

    chmod +x "$TMP_FILE"

    # Back up existing installation
    if [ -f "$TARGET" ]; then
        warn "Found existing $TARGET — backing up to ${TARGET}.bak"
        mv "$TARGET" "${TARGET}.bak"
    fi

    mkdir -p "$INSTALL_DIR"
    mv "$TMP_FILE" "$TARGET"

    success "Installed to $TARGET"
}

# ── Verify ────────────────────────────────────────────────────────────────────

verify_install() {
    TARGET="$INSTALL_DIR/$BINARY_NAME"

    if ! "$TARGET" --help > /dev/null 2>&1; then
        warn "Binary installed but failed to run. Check architecture compatibility."
        return
    fi

    success "Binary verified OK"
}

# ── Post-install guidance ─────────────────────────────────────────────────────

print_next_steps() {
    printf "\n"
    printf "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"
    printf "${GREEN}  MemX installed successfully!${NC}\n"
    printf "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"
    printf "\n"
    printf "${BOLD}Next steps:${NC}\n"
    printf "\n"
    if [ "$CONFIG_CREATED" -eq 1 ]; then
        printf "  Config written to:\n"
        printf "     ${YELLOW}%s${NC}\n" "$CONFIG_PATH"
        printf "\n"
        printf "  1. Start the server:\n"
        printf "     ${YELLOW}memx serve${NC}\n"
        printf "\n"
        printf "  2. Or use the CLI directly (no server needed):\n"
        printf "     ${YELLOW}memx add \"I prefer 4-space indentation in Rust\"${NC}\n"
        printf "     ${YELLOW}memx search \"code style\"${NC}\n"
        printf "     ${YELLOW}memx list${NC}\n"
    else
        if [ "$CONFIG_SKIPPED" -eq 1 ]; then
            printf "  Config was not created during install.\n"
            printf "\n"
        fi
        printf "  1. Edit the config and add your embedding settings:\n"
        printf "     ${YELLOW}vim %s${NC}\n" "$CONFIG_PATH"
        printf "\n"
        printf "     Minimum required:\n"
        printf "       [embedding]\n"
        printf "       api_key = \"your-api-key\"\n"
        printf "       base_url = \"%s\"\n" "$DEFAULT_EMBEDDING_BASE_URL"
        printf "       model = \"%s\"\n" "$DEFAULT_EMBEDDING_MODEL"
        printf "       dimension = %s\n" "$DEFAULT_EMBEDDING_DIMENSION"
        printf "\n"
        printf "  2. Start the server:\n"
        printf "     ${YELLOW}memx serve${NC}\n"
        printf "\n"
        printf "  3. Or use the CLI directly (no server needed):\n"
        printf "     ${YELLOW}memx add \"I prefer 4-space indentation in Rust\"${NC}\n"
        printf "     ${YELLOW}memx search \"code style\"${NC}\n"
        printf "     ${YELLOW}memx list${NC}\n"
    fi
    printf "\n"
    printf "  Docs: ${BOLD}https://memx.me${NC}\n"
    printf "\n"
}

# ── Main ──────────────────────────────────────────────────────────────────────

main() {
    printf "\n"
    info "MemX Installer"
    printf "\n"

    check_root
    need_cmd uname
    need_cmd awk
    need_cmd sed
    setup_prompt_io

    OS=$(detect_os)
    ARCH=$(detect_arch)
    detect_config_target

    # Create temp workspace; clean up on any exit
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

    download_binary "$OS" "$ARCH"
    install_binary
    verify_install
    configure_memx
    print_next_steps
}

main "$@"

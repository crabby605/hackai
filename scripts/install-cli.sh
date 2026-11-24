#!/usr/bin/env bash
# HackAI CLI Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/scripts/install-cli.sh | bash
#        curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/scripts/install-cli.sh | bash -s -- --version v0.1.0

set -euo pipefail

# Configuration
REPO="crabby605/hackai"
BINARY_NAME="hackai"
BINARY_FILE_NAME="hackai"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
GITHUB_API="https://api.github.com/repos/${REPO}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
log_info() { echo -e "${BLUE}[info]${NC} $*" >&2; }
log_success() { echo -e "${GREEN}[success]${NC} $*" >&2; }
log_warn() { echo -e "${YELLOW}[warn]${NC} $*" >&2; }
log_error() { echo -e "${RED}[error]${NC} $*" >&2; }

usage() {
    cat <<EOF
HackAI CLI Installer

Usage: $0 [options]

Options:
    --version VERSION    Install specific version (default: latest)
    --dir DIR           Installation directory (default: ~/.local/bin)
    --force             Overwrite existing installation
    --login             Start Hack Club device authorization after installation
    --dry-run           Show what would be done without executing
    --help              Show this help

Environment variables:
    INSTALL_DIR         Installation directory (default: ~/.local/bin)
    HACKAI_SERVER_URL   Server URL for the CLI (default: https://hackai.crabby605.hackclub.app)

Examples:
    # Install latest release
    curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/scripts/install-cli.sh | bash

    # Install specific version
    curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/scripts/install-cli.sh | bash -s -- --version v0.1.0

    # Custom install directory
    curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/scripts/install-cli.sh | INSTALL_DIR=/usr/local/bin bash

    # Install and authenticate the current repository
    curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/scripts/install-cli.sh | bash -s -- --login

EOF
}

# Parse arguments
VERSION=""
FORCE=false
DRY_RUN=false
LOGIN=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --version)
            VERSION="$2"
            shift 2
            ;;
        --dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --force)
            FORCE=true
            shift
            ;;
        --login)
            LOGIN=true
            shift
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --help)
            usage
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

# Detect platform
detect_platform() {
    local os arch
    case "$(uname -s)" in
        Linux*)     os="linux" ;;
        Darwin*)    os="macos" ;;
        CYGWIN*|MINGW*|MSYS*) os="windows" ;;
        *)          log_error "Unsupported OS: $(uname -s)"; exit 1 ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)   arch="x86_64" ;;
        arm64|aarch64)  arch="aarch64" ;;
        *)              log_error "Unsupported architecture: $(uname -m)"; exit 1 ;;
    esac

    [[ "$os" == "windows" ]] && BINARY_FILE_NAME="${BINARY_NAME}.exe"
    echo "${os}-${arch}"
}

# Get latest release version from GitHub
get_latest_version() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "${GITHUB_API}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "${GITHUB_API}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
    else
        log_error "curl or wget required"
        exit 1
    fi
}

# Download and verify
download_binary() {
    local version="$1"
    local platform="$2"
    local ext="tar.gz"
    [[ "$platform" == windows-* ]] && ext="zip"

    # Release CLI publishes the stable installer names below. Keep a legacy
    # target fallback so older tags remain installable during migration.
    local asset_name="${BINARY_NAME}-${platform}.${ext}"
    local legacy_platform=""
    case "$platform" in
        linux-x86_64) legacy_platform="x86_64-unknown-linux-gnu" ;;
        linux-aarch64) legacy_platform="aarch64-unknown-linux-gnu" ;;
        macos-x86_64) legacy_platform="x86_64-apple-darwin" ;;
        macos-aarch64) legacy_platform="aarch64-apple-darwin" ;;
    esac
    local legacy_asset_name="${BINARY_NAME}-${legacy_platform}.${ext}"
    local url="https://github.com/${REPO}/releases/download/${version}/${asset_name}"
    local checksum_url="${url}.sha256"

    log_info "Downloading ${asset_name} from ${url}"

    local tmp_dir
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "${tmp_dir:-}"' EXIT

    if command -v curl >/dev/null 2>&1; then
        if ! curl -fsSL -o "${tmp_dir}/${asset_name}" "$url"; then
            [[ -n "$legacy_platform" ]] || exit 1
            asset_name="$legacy_asset_name"
            url="https://github.com/${REPO}/releases/download/${version}/${asset_name}"
            checksum_url="${url}.sha256"
            curl -fsSL -o "${tmp_dir}/${asset_name}" "$url"
        fi
        curl -fsSL -o "${tmp_dir}/${asset_name}.sha256" "$checksum_url" 2>/dev/null || true
    elif command -v wget >/dev/null 2>&1; then
        if ! wget -q -O "${tmp_dir}/${asset_name}" "$url"; then
            [[ -n "$legacy_platform" ]] || exit 1
            asset_name="$legacy_asset_name"
            url="https://github.com/${REPO}/releases/download/${version}/${asset_name}"
            checksum_url="${url}.sha256"
            wget -q -O "${tmp_dir}/${asset_name}" "$url"
        fi
        wget -q -O "${tmp_dir}/${asset_name}.sha256" "$checksum_url" 2>/dev/null || true
    else
        log_error "curl or wget required"
        exit 1
    fi

    # Verify checksum if available
    if [[ -f "${tmp_dir}/${asset_name}.sha256" ]]; then
        log_info "Verifying checksum..."
        local expected actual
        expected=$(cat "${tmp_dir}/${asset_name}.sha256" | awk '{print $1}')
        if command -v sha256sum >/dev/null 2>&1; then
            actual=$(sha256sum "${tmp_dir}/${asset_name}" | awk '{print $1}')
        elif command -v shasum >/dev/null 2>&1; then
            actual=$(shasum -a 256 "${tmp_dir}/${asset_name}" | awk '{print $1}')
        else
            log_warn "No sha256 tool found, skipping verification"
        fi
        if [[ -n "${actual:-}" && "$expected" != "$actual" ]]; then
            log_error "Checksum mismatch! Expected: $expected, Got: $actual"
            exit 1
        fi
        log_success "Checksum verified"
    else
        log_warn "No checksum file found, skipping verification"
    fi

    # Extract
    log_info "Extracting..."
    if [[ "$ext" == "tar.gz" ]]; then
        tar -xzf "${tmp_dir}/${asset_name}" -C "${tmp_dir}"
    else
        unzip -q "${tmp_dir}/${asset_name}" -d "${tmp_dir}"
    fi

    # Find binary
    local binary_path
    binary_path=$(find "${tmp_dir}" \( -name "${BINARY_NAME}" -o -name "${BINARY_NAME}.exe" \) -type f | head -1)
    if [[ -z "$binary_path" ]]; then
        log_error "Binary not found in archive"
        exit 1
    fi

    echo "$binary_path"
}

# Install binary
install_binary() {
    local binary_path="$1"
    local target_dir="$2"

    if [[ ! -d "$target_dir" ]]; then
        log_info "Creating directory: $target_dir"
        if [[ "$DRY_RUN" == "true" ]]; then
            log_info "[dry-run] mkdir -p $target_dir"
        else
            mkdir -p "$target_dir"
        fi
    fi

    local target_path="${target_dir}/${BINARY_FILE_NAME}"

    if [[ -f "$target_path" && "$FORCE" != "true" ]]; then
        log_error "Binary already exists at $target_path. Use --force to overwrite."
        exit 1
    fi

    log_info "Installing to $target_path"
    if [[ "$DRY_RUN" == "true" ]]; then
        log_info "[dry-run] cp $binary_path $target_path"
        log_info "[dry-run] chmod +x $target_path"
        log_info "[dry-run] touch ${target_path}.release"
    else
        cp "$binary_path" "$target_path"
        chmod +x "$target_path"
        # Marker file so `hackai update` knows this binary was installed by the
        # release installer and is eligible for self-update.
        touch "${target_path}.release"
    fi
}

# Verify installation
verify_installation() {
    local target_dir="$1"
    local target_path="${target_dir}/${BINARY_FILE_NAME}"

    if [[ "$DRY_RUN" == "true" ]]; then
        log_info "[dry-run] would verify installation"
        return 0
    fi

    if [[ ! -x "$target_path" ]]; then
        log_error "Installation failed: $target_path not executable"
        exit 1
    fi

    # Test binary runs
    if "$target_path" --version >/dev/null 2>&1; then
        local version_output
        version_output=$("$target_path" --version 2>/dev/null || echo "unknown")
        log_success "Installation verified: $version_output"
    else
        log_warn "Binary installed but --version check failed (may be expected for dev builds)"
    fi
}

# Check PATH
check_path() {
    local target_dir="$1"
    case ":$PATH:" in
        *":$target_dir:"*) ;;
        *)
            log_warn "Installation directory $target_dir is not in your PATH"
            log_warn "Add it to your shell config:"
            log_warn "  export PATH=\"$target_dir:\$PATH\""
            ;;
    esac
}

# Main
main() {
    log_info "HackAI CLI Installer"

    local platform
    platform=$(detect_platform)
    [[ "$platform" == windows-* ]] && BINARY_FILE_NAME="${BINARY_NAME}.exe"
    log_info "Platform: $platform"

    if [[ -z "$VERSION" && "$DRY_RUN" == "true" ]]; then
        VERSION="<latest>"
        log_info "[dry-run] would fetch the latest GitHub release version"
    elif [[ -z "$VERSION" ]]; then
        log_info "Fetching latest release version..."
        VERSION=$(get_latest_version)
        if [[ -z "$VERSION" ]]; then
            log_error "Failed to fetch latest version"
            exit 1
        fi
    fi
    log_info "Version: $VERSION"

    # Check if already installed
    local existing_version=""
    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        existing_version=$("$BINARY_NAME" --version 2>/dev/null || echo "unknown")
        log_info "Existing installation: $existing_version"
        if [[ "$FORCE" != "true" && "$existing_version" == *"$VERSION"* ]]; then
            log_success "Already up to date"
            exit 0
        fi
    fi

    if [[ "$DRY_RUN" == "true" ]]; then
        local ext="tar.gz"
        [[ "$platform" == windows-* ]] && ext="zip"
        log_info "[dry-run] would download hackai-${platform}.${ext} from GitHub release ${VERSION}"
        log_info "[dry-run] would install ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"
        check_path "$INSTALL_DIR"
        return 0
    fi

    # Download
    local binary_path
    binary_path=$(download_binary "$VERSION" "$platform")

    # Install
    install_binary "$binary_path" "$INSTALL_DIR"

    # Verify
    verify_installation "$INSTALL_DIR"

    # PATH check
    check_path "$INSTALL_DIR"

    log_success "HackAI CLI $VERSION installed successfully!"
    echo
    echo "Next steps:"
    echo "  1. Run 'hackai --help' to see available commands"
    echo "  2. Run 'hackai login' in a git repository to authenticate with the server"
    echo "  3. Run 'hackai track' in a git repository to start tracking"
    echo
    echo "Server URL: ${HACKAI_SERVER_URL:-https://hackai.crabby605.hackclub.app}"

    if [[ "$LOGIN" == "true" ]]; then
        if command -v git >/dev/null 2>&1 && git rev-parse --show-toplevel >/dev/null 2>&1; then
            log_info "Starting Hack Club device authorization..."
            "${INSTALL_DIR}/${BINARY_NAME}" login
        else
            log_warn "Skipping login: run the installer from a git repository, then use 'hackai login'."
        fi
    fi
}

main "$@"

#!/usr/bin/env bash
# hackai installer - downloads and installs the latest release
# Usage: curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/scripts/install.sh | bash

set -euo pipefail

REPO="crabby605/hackai"
BINARY_NAME="hackai"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
err() { echo -e "${RED}[ERR]${NC} $*" >&2; }

# Detect platform
detect_platform() {
    local os arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        darwin)
            case "$arch" in
                arm64) echo "macos-aarch64" ;;
                x86_64) echo "macos-x86_64" ;;
                *) err "Unsupported macOS arch: $arch"; exit 1 ;;
            esac
            ;;
        linux)
            case "$arch" in
                x86_64) echo "linux-x86_64" ;;
                aarch64|arm64) echo "linux-aarch64" ;;
                *) err "Unsupported Linux arch: $arch"; exit 1 ;;
            esac
            ;;
        *) err "Unsupported OS: $os"; exit 1 ;;
    esac
}

# Get latest release tag
get_latest_tag() {
    curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
}

# Download and verify
download_binary() {
    local target="$1"
    local tag="$2"
    local url="https://github.com/$REPO/releases/download/$tag/hackai-$target.tar.gz"
    local sha256_url="https://github.com/$REPO/releases/download/$tag/hackai-$target.tar.gz.sha256"

    info "Downloading $url"
    curl -fsSL "$url" -o "${TMP_DIR}/hackai.tar.gz"
    curl -fsSL "$sha256_url" -o "${TMP_DIR}/hackai.tar.gz.sha256"

    info "Verifying checksum..."
    local expected actual
    expected=$(cut -d' ' -f1 < "${TMP_DIR}/hackai.tar.gz.sha256")
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "${TMP_DIR}/hackai.tar.gz" | cut -d' ' -f1)
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "${TMP_DIR}/hackai.tar.gz" | cut -d' ' -f1)
    else
        err "No SHA-256 tool found; refusing to install"
        exit 1
    fi
    if [[ "$expected" != "$actual" ]]; then
        err "Checksum mismatch! Expected: $expected, Got: $actual"
        exit 1
    fi
    info "Checksum verified"

    tar -xzf "${TMP_DIR}/hackai.tar.gz" -C "${TMP_DIR}"
    chmod +x "${TMP_DIR}/hackai"
}

install_binary() {
    local binary_path="$1"
    mkdir -p "$INSTALL_DIR"
    mv "$binary_path" "$INSTALL_DIR/$BINARY_NAME"
    touch "${INSTALL_DIR}/${BINARY_NAME}.release"
    info "Installed to $INSTALL_DIR/$BINARY_NAME"

    # Check if in PATH
    if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
        warn "$INSTALL_DIR not in PATH. Add to your shell config:"
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
    fi
}

main() {
    info "hackai installer"
    info "Repo: $REPO"

    local target
    target=$(detect_platform)
    info "Platform: $target"

    local tag
    tag=$(get_latest_tag)
    info "Latest release: $tag"

    download_binary "$target" "$tag"
    install_binary "${TMP_DIR}/hackai"

    info "Done! Run 'hackai --help' to get started"
    info "Configure with: hackai login"
}

main "$@"
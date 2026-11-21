#!/usr/bin/env bash
# hackai installer - downloads and installs the latest release
# Usage: curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/scripts/install.sh | bash

set -euo pipefail

REPO="crabby605/hackai"
BINARY_NAME="hackai"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

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
                arm64) echo "aarch64-apple-darwin" ;;
                x86_64) echo "x86_64-apple-darwin" ;;
                *) err "Unsupported macOS arch: $arch"; exit 1 ;;
            esac
            ;;
        linux)
            case "$arch" in
                x86_64) echo "x86_64-unknown-linux-gnu" ;;
                aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
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
    curl -fsSL "$url" -o "/tmp/hackai.tar.gz"

    if curl -fsSL "$sha256_url" -o "/tmp/hackai.tar.gz.sha256" 2>/dev/null; then
        info "Verifying checksum..."
        local expected=$(cut -d' ' -f1 < /tmp/hackai.tar.gz.sha256)
        local actual=$(sha256sum /tmp/hackai.tar.gz | cut -d' ' -f1)
        if [[ "$expected" != "$actual" ]]; then
            err "Checksum mismatch! Expected: $expected, Got: $actual"
            exit 1
        fi
        info "Checksum verified"
    else
        warn "No checksum file found, skipping verification"
    fi

    tar -xzf /tmp/hackai.tar.gz -C /tmp
    mv /tmp/hackai /tmp/$BINARY_NAME
    chmod +x /tmp/$BINARY_NAME
}

install_binary() {
    local binary_path="$1"
    mkdir -p "$INSTALL_DIR"
    mv "$binary_path" "$INSTALL_DIR/$BINARY_NAME"
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
    install_binary "/tmp/$BINARY_NAME"

    info "Done! Run 'hackai --help' to get started"
    info "Configure with: hackai login"
}

main "$@"
#!/bin/bash
# HackAI installer - downloads and installs the latest release
# Usage: curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/install.sh | bash

set -euo pipefail

REPO="crabby605/hackai"
BINARY="hackai"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  linux)
    case "$ARCH" in
      x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
      *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  darwin)
    case "$ARCH" in
      x86_64) TARGET="x86_64-apple-darwin" ;;
      arm64) TARGET="aarch64-apple-darwin" ;;
      *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS"
    exit 1
    ;;
esac

# Get latest release version
VERSION=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$VERSION" ]; then
  echo "Failed to fetch latest version"
  exit 1
fi

URL="https://github.com/$REPO/releases/download/$VERSION/hackai-$TARGET"

echo "Installing $BINARY $VERSION for $TARGET..."

# Create temp directory
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# Download binary
curl -fsSL "$URL" -o "$TMPDIR/$BINARY"
chmod +x "$TMPDIR/$BINARY"

# Install to ~/.local/bin (or /usr/local/bin if root)
INSTALL_DIR="${HOME}/.local/bin"
if [ "$(id -u)" = "0" ]; then
  INSTALL_DIR="/usr/local/bin"
fi
mkdir -p "$INSTALL_DIR"
mv "$TMPDIR/$BINARY" "$INSTALL_DIR/$BINARY"

# Add to PATH if needed
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Add $INSTALL_DIR to your PATH to use $BINARY" ;;
esac

echo "Installed $BINARY to $INSTALL_DIR"
echo "Run 'hackai --help' to get started"
#!/bin/bash
# HackAI installer - downloads and installs the latest release
# Usage: curl -fsSL https://raw.githubusercontent.com/crabby605/hackai/main/install.sh | bash

set -euo pipefail

REPO="crabby605/hackai"
BINARY="hackai"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

# Artifact names must match the release-cli build artifacts (release-cli.yml).
case "$OS" in
  linux)
    case "$ARCH" in
      x86_64) ARTIFACT="hackai-linux-x86_64" ;;
      aarch64|arm64) ARTIFACT="hackai-linux-aarch64" ;;
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  darwin)
    case "$ARCH" in
      x86_64) ARTIFACT="hackai-macos-x86_64" ;;
      arm64) ARTIFACT="hackai-macos-aarch64" ;;
      *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

# Get latest release version
VERSION=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$VERSION" ]; then
  echo "Failed to fetch latest version" >&2
  exit 1
fi

URL="https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT"
CHECKSUM_URL="${URL}.sha256"

echo "Installing $BINARY $VERSION for $ARTIFACT..."

# Create temp directory
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# Download binary
curl -fsSL "$URL" -o "$TMPDIR/$BINARY"

# Verify SHA-256 checksum (required — refuse to install without it)
curl -fsSL "$CHECKSUM_URL" -o "$TMPDIR/$BINARY.sha256"
expected=$(awk '{print $1}' "$TMPDIR/$BINARY.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$TMPDIR/$BINARY" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$TMPDIR/$BINARY" | awk '{print $1}')
else
  echo "No sha256 tool found, refusing to install without checksum verification" >&2
  exit 1
fi
if [ "$expected" != "$actual" ]; then
  echo "Checksum mismatch: expected $expected, got $actual" >&2
  exit 1
fi

chmod +x "$TMPDIR/$BINARY"

# Install to ~/.local/bin (or /usr/local/bin if root)
INSTALL_DIR="${HOME}/.local/bin"
if [ "$(id -u)" = "0" ]; then
  INSTALL_DIR="/usr/local/bin"
fi
mkdir -p "$INSTALL_DIR"
mv "$TMPDIR/$BINARY" "$INSTALL_DIR/$BINARY"

# Marker file so `hackai update` knows this binary was installed by the
# release installer and is eligible for self-update.
touch "${INSTALL_DIR}/${BINARY}.release"

# Add to PATH if needed
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Add $INSTALL_DIR to your PATH to use $BINARY" ;;
esac

echo "Installed $BINARY to $INSTALL_DIR"
echo "Run 'hackai --help' to get started"

#!/bin/sh
set -e

REPO="worktoolai/jsonai"
NAME="jsonai"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.worktoolai/bin}"

# Detect OS
OS="$(uname -s)"
case "$OS" in
  Linux*)  OS="linux" ;;
  Darwin*) OS="darwin" ;;
  *)       echo "Unsupported OS: $OS"; exit 1 ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)  ARCH="amd64" ;;
  aarch64|arm64)  ARCH="arm64" ;;
  *)              echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

ARTIFACT="${NAME}-${OS}-${ARCH}"
echo "Installing ${NAME} (${OS}/${ARCH})..."

# Get latest release tag
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
if [ -z "$TAG" ]; then
  echo "Failed to fetch latest release"; exit 1
fi
echo "Latest release: ${TAG}"

# Download
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARTIFACT}"
TMP=$(mktemp)
curl -fsSL -o "$TMP" "$URL"
if [ ! -s "$TMP" ]; then
  echo "Download failed: ${URL}"; rm -f "$TMP"; exit 1
fi

# Install
mkdir -p "$INSTALL_DIR"
chmod +x "$TMP"
mv "$TMP" "${INSTALL_DIR}/${NAME}"

echo "Installed ${NAME} ${TAG} to ${INSTALL_DIR}/${NAME}"

# Add to PATH if not already present
PATH_LINE='export PATH="$HOME/.worktoolai/bin:$PATH"'
case ":$PATH:" in
  *":${INSTALL_DIR}:"*)
    echo "PATH already configured."
    ;;
  *)
    for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile"; do
      if [ -f "$rc" ]; then
        if ! grep -qF '.worktoolai/bin' "$rc"; then
          echo "" >> "$rc"
          echo "$PATH_LINE" >> "$rc"
          echo "Added PATH to ${rc}"
        fi
      fi
    done
    echo ""
    echo "Restart your shell or run:"
    echo "  export PATH=\"\$HOME/.worktoolai/bin:\$PATH\""
    ;;
esac

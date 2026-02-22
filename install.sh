#!/bin/sh
set -e

REPO="worktoolai/jsonai"
NAME="jsonai"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

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
chmod +x "$TMP"
if [ -w "$INSTALL_DIR" ]; then
  mv "$TMP" "${INSTALL_DIR}/${NAME}"
else
  echo "Need sudo to install to ${INSTALL_DIR}"
  sudo mv "$TMP" "${INSTALL_DIR}/${NAME}"
fi

echo "Installed ${NAME} ${TAG} to ${INSTALL_DIR}/${NAME}"

#!/bin/sh
# Orchyn CLI Installer
# Usage: curl -sSf https://raw.githubusercontent.com/your-org/orchyn/main/install.sh | sh

set -e

REPO="your-org/orchyn"
BIN_NAME="orchyn-cli"
INSTALL_DIR="${HOME}/.local/bin"
VERSION="latest"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m'

log_info() {
    printf "${BLUE}==>${NC} %s\n" "$1"
}

log_success() {
    printf "${GREEN}==>${NC} %s\n" "$1"
}

log_error() {
    printf "${RED}==>${NC} %s\n" "$1"
}

detect_os_arch() {
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    ARCH="$(uname -m)"

    case "$OS" in
        linux)
            OS="unknown-linux-gnu"
            EXE=""
            ;;
        darwin)
            OS="apple-darwin"
            EXE=""
            ;;
        mingw*|msys*|cygwin*)
            OS="pc-windows-msvc"
            EXE=".exe"
            ;;
        *)
            log_error "Unsupported OS: $OS"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64)
            ARCH="x86_64"
            ;;
        aarch64|arm64)
            ARCH="aarch64"
            ;;
        *)
            log_error "Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac

    TARGET="${ARCH}-${OS}"
}

install_from_source() {
    log_info "Attempting to build from source using cargo..."
    if command -v cargo >/dev/null 2>&1; then
        cargo install --git "https://github.com/${REPO}" orchyn-cli
        log_success "Orchyn CLI installed successfully via cargo."
        exit 0
    else
        log_error "Cargo is not installed. Please install Rust (https://rustup.rs) or download the binary manually."
        exit 1
    fi
}

main() {
    log_info "Installing Orchyn CLI..."
    
    detect_os_arch
    
    # Ensure install directory exists
    mkdir -p "$INSTALL_DIR"

    # In a real release environment, we would fetch the latest tag from GitHub API:
    # URL="https://github.com/${REPO}/releases/latest/download/${BIN_NAME}-${TARGET}.tar.gz"
    # For now, since the repo might not have binary releases yet, we default to source build.
    
    log_info "Detected target: $TARGET"
    
    # Placeholder for binary download logic
    # if curl -sLf "$URL" -o orchyn.tar.gz; then
    #     tar -xzf orchyn.tar.gz
    #     mv "$BIN_NAME" "$INSTALL_DIR/"
    #     rm orchyn.tar.gz
    #     log_success "Successfully installed to $INSTALL_DIR/$BIN_NAME"
    # else
    #     log_info "Pre-built binary not found for $TARGET. Falling back to source build."
    #     install_from_source
    # fi
    
    install_from_source

    # Check if INSTALL_DIR is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            log_info "Note: $INSTALL_DIR is not in your PATH."
            log_info "Add the following to your ~/.bashrc or ~/.zshrc:"
            echo "export PATH=\"$INSTALL_DIR:\$PATH\""
            ;;
    esac
}

main

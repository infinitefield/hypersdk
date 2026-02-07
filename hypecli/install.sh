#!/bin/sh
# hypecli installer
# Usage: curl -fsSL https://raw.githubusercontent.com/infinitefield/hypersdk/main/hypecli/install.sh | sh

set -e

REPO="infinitefield/hypersdk"
BINARY="hypecli"

# Detect OS and architecture
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)
            PLATFORM_OS="linux"
            DEFAULT_INSTALL_DIR="/usr/local/bin"
            ;;
        Darwin)
            PLATFORM_OS="macos"
            DEFAULT_INSTALL_DIR="$HOME/.local/bin"
            ;;
        *)
            echo "Error: Unsupported operating system: $OS"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64|x64)
            PLATFORM_ARCH="x86_64"
            ;;
        arm64|aarch64|armv8*|arm8*)
            PLATFORM_ARCH="aarch64"
            ;;
        *)
            echo "Error: Unsupported architecture: $ARCH"
            echo "Only x86_64 or aarch64 are available"
            exit 1
            ;;
    esac

    TARGET="${PLATFORM_OS}-${PLATFORM_ARCH}"
    INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
}

# Get the latest release version
get_latest_version() {
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$VERSION" ]; then
        echo "Error: Could not determine latest version"
        exit 1
    fi
}

# Download and install
install() {
    detect_platform
    get_latest_version

    echo "Installing ${BINARY} ${VERSION} for ${TARGET}..."

    # Construct download URL (matches GitHub releases asset naming)
    ASSET_NAME="${BINARY}-${TARGET}.tar.gz"
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET_NAME}"

    # Create temp directory
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    # Download binary
    echo "Downloading from ${DOWNLOAD_URL}..."
    if ! curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${ASSET_NAME}"; then
        echo "Error: Failed to download ${BINARY}"
        echo ""
        echo "Release assets may not be available for ${TARGET}."
        exit 1
    fi

    # Extract
    echo "Extracting..."
    tar -xzf "${TMP_DIR}/${ASSET_NAME}" -C "${TMP_DIR}"

    # Make executable
    chmod +x "${TMP_DIR}/${BINARY}"

    # Ensure install directory exists
    mkdir -p "$INSTALL_DIR"

    # Install
    if [ -w "$INSTALL_DIR" ]; then
        mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    else
        echo "Installing to ${INSTALL_DIR} requires sudo..."
        sudo mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    fi

    echo ""
    echo "Successfully installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

    # Check if install dir is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo ""
            echo "Note: ${INSTALL_DIR} is not in your PATH."
            echo "Add it to your shell profile:"
            echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
            ;;
    esac

    echo ""
    echo "Run '${BINARY} --help' to get started"
    echo "Run '${BINARY} --agent-help' for detailed AI agent documentation"
}

# Main
main() {
    echo "hypecli installer"
    echo "================="
    echo ""
    install
}

main "$@"

#!/bin/bash
# Build script for roslyn-wrapper
# This script builds the binary and copies it to the locations needed for local testing

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
TARGET_DIR="${PROJECT_ROOT}/target"

echo "🔨 Building roslyn-wrapper..."
cargo build --release

# Detect platform
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    BINARY_NAME="roslyn-wrapper"
    PLATFORM="x86_64-unknown-linux-gnu"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    BINARY_NAME="roslyn-wrapper"
    if [[ $(uname -m) == "arm64" ]]; then
        PLATFORM="aarch64-apple-darwin"
    else
        PLATFORM="x86_64-apple-darwin"
    fi
else
    echo "❌ Unsupported platform: $OSTYPE"
    exit 1
fi

BINARY_PATH="${TARGET_DIR}/release/${BINARY_NAME}"

if [ ! -f "$BINARY_PATH" ]; then
    echo "❌ Binary not found at $BINARY_PATH"
    exit 1
fi

echo "✅ Binary built successfully: $BINARY_PATH"
echo ""

# Copy to cache locations for local testing
echo "📦 Copying binary to local cache locations for testing..."

# Create cache directories
mkdir -p ~/.local/share/roslyn-wrapper/bin/0.1.0
mkdir -p ~/.cache/roslyn-wrapper/bin/0.1.0

# Copy binary
cp "$BINARY_PATH" ~/.local/share/roslyn-wrapper/bin/0.1.0/
cp "$BINARY_PATH" ~/.cache/roslyn-wrapper/bin/0.1.0/

# Make executable
chmod +x ~/.local/share/roslyn-wrapper/bin/0.1.0/$BINARY_NAME
chmod +x ~/.cache/roslyn-wrapper/bin/0.1.0/$BINARY_NAME

echo ""
echo "✅ Build complete!"
echo "📍 Binary location: $BINARY_PATH"
echo "📁 Cached at: ~/.local/share/roslyn-wrapper/bin/0.1.0/$BINARY_NAME"
echo "📁 Cached at: ~/.cache/roslyn-wrapper/bin/0.1.0/$BINARY_NAME"

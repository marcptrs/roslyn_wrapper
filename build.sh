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

# Copy to Zed extension cache for local testing
echo "📦 Copying binary to Zed extension cache for testing..."

# Detect OS and set cache path
if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS
    ZED_CACHE_DIR="${HOME}/Library/Application Support/Zed/extensions/work/csharp_roslyn"
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    # Linux
    ZED_CACHE_DIR="${HOME}/.config/zed/extensions/work/csharp_roslyn"
else
    echo "❌ Unsupported platform: $OSTYPE"
    exit 1
fi

# Use a fixed cache directory (no versioning in path for simpler updates)
CACHE_DIR="${ZED_CACHE_DIR}/roslyn-wrapper"
mkdir -p "$CACHE_DIR"

# Copy binary
cp "$BINARY_PATH" "$CACHE_DIR/"

# Make executable
chmod +x "$CACHE_DIR/$BINARY_NAME"

echo ""
echo "✅ Build complete!"
echo "📍 Binary location: $BINARY_PATH"
echo "📁 Zed cache location: $CACHE_DIR/$BINARY_NAME"
echo "🎯 The extension will automatically use this cached binary when running in Zed"

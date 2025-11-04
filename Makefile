.PHONY: build build-release build-debug clean test help install

help:
	@echo "roslyn-wrapper build targets:"
	@echo ""
	@echo "  make build          - Build release binary (default)"
	@echo "  make build-release  - Build optimized release binary"
	@echo "  make build-debug    - Build debug binary with debug info"
	@echo "  make clean          - Remove all build artifacts"
	@echo "  make install        - Build and install to cache directories"
	@echo "  make test           - Run tests"
	@echo "  make help           - Show this help message"

build: build-release

build-release:
	@echo "🔨 Building roslyn-wrapper (release)..."
	cargo build --release

build-debug:
	@echo "🔨 Building roslyn-wrapper (debug)..."
	cargo build

clean:
	@echo "🧹 Cleaning build artifacts..."
	cargo clean

test:
	@echo "🧪 Running tests..."
	cargo test

install: build-release
	@echo "📦 Installing binary to cache directories..."
	@mkdir -p ~/.local/share/roslyn-wrapper/bin/0.1.0
	@mkdir -p ~/.cache/roslyn-wrapper/bin/0.1.0
	@cp target/release/roslyn-wrapper ~/.local/share/roslyn-wrapper/bin/0.1.0/
	@cp target/release/roslyn-wrapper ~/.cache/roslyn-wrapper/bin/0.1.0/
	@chmod +x ~/.local/share/roslyn-wrapper/bin/0.1.0/roslyn-wrapper
	@chmod +x ~/.cache/roslyn-wrapper/bin/0.1.0/roslyn-wrapper
	@echo "✅ Installed successfully!"
	@echo "📁 Cached at: ~/.local/share/roslyn-wrapper/bin/0.1.0/roslyn-wrapper"
	@echo "📁 Cached at: ~/.cache/roslyn-wrapper/bin/0.1.0/roslyn-wrapper"

# AGENTS.md - Agentic Coding Guidelines

## Build & Test Commands

```bash
# Build release binary
cargo build --release

# Build debug binary
cargo build

# Run all tests
cargo test

# Run a single test (e.g., testing message parsing)
cargo test test_name -- --nocapture --test-threads=1

# Run tests with logging
RUST_LOG=debug cargo test -- --nocapture

# Check code without building
cargo check

# Format code
cargo fmt

# Lint with clippy
cargo clippy -- -D warnings
```

## Code Style Guidelines

### Imports
- Group imports: standard library, external crates, internal modules
- Use `use` statements; avoid glob imports except in tests
- Each module in `src/` should declare mods at top before imports

### Formatting & Conventions
- Follow Rust 2021 edition idioms (use `?` operator, avoid `unwrap()` where error handling matters)
- Max args: 8 (enforced by `clippy.toml`); split multi-arg functions
- Constants in UPPER_SNAKE_CASE (e.g., `LSP_MESSAGE_TYPE_ERROR`)
- Functions/variables in snake_case, types/traits in PascalCase
- Lines should typically fit in standard editor width (no hard limit enforced)

### Error Handling
- Prefer `Result<T>` with `anyhow::Result` for errors that get logged
- Use `?` operator for error propagation; unwrap only for infallible operations
- Log errors before returning them: `logger::error(format!("[roslyn_wrapper] ..."))`
- Never silently drop errors without logging

### Functions & Documentation
- Add doc comments (`///`) for public functions explaining purpose & parameters
- For complex logic, add inline comments explaining "why", not "what"
- Keep functions focused and testable
- Use async functions (`async fn`) for I/O operations

### Naming & Logging
- All log messages must start with `[roslyn_wrapper]` for easy filtering
- Use `logger::{info, debug, error, warn}` from `crate::logger`
- Variable names should be descriptive; avoid single letters except for loop iterators

### Types & Serialization
- Use `serde_json::Value` for LSP message handling
- Derive `serde::{Serialize, Deserialize}` on structs that need JSON conversion
- Use `PathBuf` (not `&str`) for file paths; leverage `path_utils` module for operations

### Architecture
- `main.rs`: LSP proxy core, message forwarding, async orchestration
- `download.rs`: Roslyn binary management & caching (uses `anyhow::Result`)
- `logger.rs`: Centralized logging (all messages logged to disk)
- `path_utils.rs`: Cross-platform path utilities

### Testing
- Tests live inline with `#[cfg(test)]` modules
- Use `tokio::test` for async tests
- Mock file systems with `tempfile` crate
- Keep test names descriptive: `test_<function>_<scenario>`

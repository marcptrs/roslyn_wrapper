# Roslyn Wrapper

A lightweight Rust-based LSP wrapper for the Roslyn language server that enables compatibility with editors beyond VSCode.

## Overview

This CLI tool acts as a transparent proxy between LSP clients (like Zed) and the Roslyn C# language server (`Microsoft.CodeAnalysis.LanguageServer`), with one critical enhancement:

- **Automatic solution/project detection**: Intercepts the LSP `initialize` → `initialized` handshake to inject a `solution/open` or `project/open` notification, enabling Roslyn to load the workspace context.
- **Transparent message forwarding**: All other LSP messages pass through unmodified using direct stdin/stdout forwarding.

## Building

```bash
cargo build --release
```

## Usage

```bash
roslyn-wrapper [--debug] <path-to-roslyn-dll> [additional-args...]
```

The wrapper is invoked by the [csharp_roslyn](https://github.com/marcptrs/csharp_roslyn) extension.

### Options

- `--debug`: Enable debug logging to `logs/proxy-debug.log` (disabled by default)

## How It Works

1. **Startup**: Spawns the Roslyn language server as a child process
2. **Forwarding**: Uses `io::copy` to forward stdin → server and server → stdout transparently
3. **Interception**: Detects the `initialize` request in the message stream
4. **Injection**: After forwarding the `initialized` notification, injects:
   - `solution/open` notification if a `.sln` or `.slnx` file is found
   - `project/open` notification if `.csproj` files are found
5. **Resume**: Returns to transparent forwarding for all subsequent messages

## Integration

This tool is downloaded automatically by the [csharp_roslyn](https://github.com/marcptrs/csharp_roslyn) extension from GitHub releases.

## Releasing

To create a new release:

1. Update `WRAPPER_VERSION` in `csharp_roslyn/src/nuget.rs`
2. Build binaries for all platforms:
   ```bash
   # macOS ARM64
   cargo build --release --target aarch64-apple-darwin

   # macOS x64
   cargo build --release --target x86_64-apple-darwin

   # Linux ARM64
   cargo build --release --target aarch64-unknown-linux-gnu

   # Linux x64
   cargo build --release --target x86_64-unknown-linux-gnu

   # Windows x64
   cargo build --release --target x86_64-pc-windows-msvc
   ```

3. Create a GitHub release with tag `v{version}` (e.g., `v0.1.0`)
4. Upload the following binaries as release assets:
   - `roslyn-wrapper-osx-arm64` (from aarch64-apple-darwin)
   - `roslyn-wrapper-osx-x64` (from x86_64-apple-darwin)
   - `roslyn-wrapper-linux-arm64` (from aarch64-unknown-linux-gnu)
   - `roslyn-wrapper-linux-x64` (from x86_64-unknown-linux-gnu)
   - `roslyn-wrapper-win-x64.exe` (from x86_64-pc-windows-msvc)

## Configuration

Update `WRAPPER_REPO_OWNER` and `WRAPPER_REPO_NAME` in `csharp_roslyn/src/nuget.rs` to point to your GitHub repository.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Inspiration

This project was inspired by [SofusA/csharp-language-server](https://github.com/SofusA/csharp-language-server), which demonstrated the transparent wrapper approach for Roslyn compatibility with non-VSCode editors.

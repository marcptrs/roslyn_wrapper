# roslyn-wrapper

A transparent LSP proxy for the C# Roslyn language server that enables cross-solution/project support in editors like Zed.

## What It Does

roslyn-wrapper acts as a transparent proxy between your editor and the Roslyn language server (`Microsoft.CodeAnalysis.LanguageServer`). It:

- Forwards all LSP messages bidirectionally
- Automatically downloads and caches the Roslyn language server
- Maps Roslyn's custom `window/_roslyn_showToast` → standard LSP `window/showMessage`
- Logs wrapper activity to a file for debugging
- Helps open multiple C# projects/solutions in one editor instance

## Installation

### Prerequisites

- Rust toolchain (to build from source)
- .NET SDK 8.0+ (for C# development)
- Internet access on first run (to download Roslyn), unless you provide a local Roslyn path

### Building

```bash
# Clone the repository
git clone https://github.com/marcptrs/roslyn-wrapper
cd roslyn-wrapper

# Build release binary
cargo build --release

# Binary will be at: target/release/roslyn-wrapper
```

### Platform-Specific Scripts

```bash
# Linux/macOS
./build.sh

# Windows
.\build.ps1
```

## Usage

### Command Line

roslyn-wrapper supports three behaviors:

- Default (proxy) mode — no arguments
  - Downloads/uses cached Roslyn and proxies over stdio.
  - Example: `roslyn-wrapper`

- Pass-through flags to Roslyn — first arg starts with `-`
  - Forwards flags like `--help`, `--version` to Roslyn and exits with its code.
  - Example: `roslyn-wrapper --version`

- Explicit Roslyn binary path — first arg is a path
  - Uses that Roslyn LSP binary instead of downloading.
  - Example: `roslyn-wrapper /path/to/Microsoft.CodeAnalysis.LanguageServer`

### Editor Integration (Zed)

Add to your Zed `settings.json` (minimal example):

```json
{
  "lsp": {
    "roslyn": {
      "binary": {
        "path": "/absolute/path/to/roslyn-wrapper"
      },
      "initialization_options": {
        "solution": "file:///absolute/path/to/YourSolution.sln"
      }
    }
  }
}
```

Behavior:
- `binary.path` is optional if the `roslyn-wrapper` binary is on your `PATH` or launched via another mechanism.
- `initialization_options.solution` is optional. If omitted, the wrapper tries to discover a `.sln` or `.csproj` under the workspace roots (from `rootUri` and/or `workspaceFolders`) up to depth 4 and sends `solution/open` if found.
- If nothing is found, it warns via `window/showMessage` that C# features are limited until a solution/project is opened.

## Logs

The wrapper writes timestamped log lines to a file.

Defaults:
- File: `./roslyn_wrapper.log` (current working directory when the process starts)
- Level: `info`

Configuration is done via LSP `initialization_options` (not environment variables). These keys are read from the `initialize` request:
- `logLevel`: `off` | `error` | `info` | `debug` (case-insensitive)
- `logFile`: absolute or relative path to the desired log file (overrides `logDirectory`)
- `logDirectory`: directory where `roslyn_wrapper.log` will be created (ignored if `logFile` is provided)

Precedence: `logFile` > `logDirectory` > default path.

Setting `logLevel: "off"` disables all logging output after the initial setup line.

Example Zed configuration with custom logging:
```json
{
  "lsp": {
    "roslyn": {
      "binary": { "path": "/absolute/path/to/roslyn-wrapper" },
      "initialization_options": {
        "solution": "file:///absolute/path/to/YourSolution.sln",
        "logLevel": "debug",
        "logDirectory": "/tmp/roslyn-logs"
      }
    }
  }
}
```

Example using an explicit file instead of a directory:
```json
"initialization_options": {
  "logFile": "/var/log/roslyn-wrapper/roslyn-wrapper.log",
  "logLevel": "error"
}
```

Tail the log in real time:
```bash
tail -f ./roslyn_wrapper.log
```

Breaking change: older versions used `ROSLYN_WRAPPER_LOG_LEVEL`, `ROSLYN_WRAPPER_LOG_PATH`, and `ROSLYN_WRAPPER_CWD`. These environment variables are no longer read. Update your editor configuration instead.

## Viewing LSP Messages in Zed

- Use the LSP Log Viewer:
  - Command Palette (Cmd/Ctrl+Shift+P) → "Open Language Server Logs"
  - Select the `roslyn` server

## Architecture

### Transparent Proxy Design

```
Editor (Zed) ←→ roslyn-wrapper ←→ Roslyn Language Server
                      ↓
                  log file
```

What the wrapper does:
- Forwards all LSP messages unchanged (except mapping `_roslyn_showToast` to `window/showMessage`)
- Logs activity for debugging

What the wrapper does not do:
- Generate custom notifications beyond the toast mapping
- Parse or modify Roslyn error payloads
- Implement custom progress indicators

### Message Flow

1. Editor → Wrapper → Roslyn
   - LSP requests (textDocument/*, workspace/*, etc.)
   - Initialization, configuration, document changes

2. Roslyn → Wrapper → Editor
   - `window/logMessage`: Progress updates, errors, warnings
   - `window/showMessage`: User-facing messages (via `_roslyn_showToast` mapping)
   - `textDocument/publishDiagnostics`: Code errors/warnings
   - `workspace/projectInitializationComplete`: Loading complete

## Troubleshooting

### "A compatible .NET SDK was not found"

1. Check your project's `global.json` for SDK version requirements
2. Install the SDK from https://dotnet.microsoft.com/download
3. Verify with `dotnet --version`
4. Restart the editor

### No Diagnostics/IntelliSense

1. Open Zed's Language Server Logs and look for errors
2. Ensure your project loads without errors
3. Check `./roslyn_wrapper.log` for wrapper/Roslyn process output
4. Verify .NET SDK is installed
5. Restart the language server in Zed

### Project Not Loading

1. Make sure a `.sln` or `.csproj` exists in the workspace
2. Check LSP logs for errors
3. Run `dotnet build` to ensure the project compiles
4. Run `dotnet restore` for missing packages

## Development

### Running Tests

```bash
cargo test
```

### Code Structure

```
src/
├── main.rs         # Entry point, LSP proxy logic, message forwarding
├── download.rs     # Roslyn language server download and management
├── logger.rs       # Logging infrastructure
└── path_utils.rs   # Path manipulation utilities
```

## License

MIT License — see [LICENSE](LICENSE).

## Related Links

- Roslyn Language Server: https://github.com/dotnet/roslyn/tree/main/src/Features/LanguageServer
- LSP Specification: https://microsoft.github.io/language-server-protocol/

## Acknowledgments

- Built for use with the [Zed](https://zed.dev/) editor
- Uses the Roslyn language server from the .NET team
- Inspired by the need for cross-solution C# support in modern editors

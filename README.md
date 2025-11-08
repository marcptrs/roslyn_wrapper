# roslyn-wrapper

A transparent LSP proxy for the C# Roslyn language server that enables cross-solution/project support in editors like Zed.

## What It Does

roslyn-wrapper acts as a **transparent proxy** between your editor and the Roslyn language server (`Microsoft.CodeAnalysis.LanguageServer.dll`). It:

- ✅ Forwards all LSP messages bidirectionally without modification
- ✅ Automatically downloads and manages the Roslyn language server
- ✅ Maps Roslyn's custom `window/_roslyn_showToast` → standard LSP `window/showMessage`
- ✅ Logs all LSP traffic to file for debugging
- ✅ Enables opening multiple C# projects/solutions in the same editor instance

## Installation

### Prerequisites

- Rust toolchain (for building from source)
- .NET SDK 8.0 or later (for C# development)

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

```bash
roslyn-wrapper [OPTIONS]

Options:
  --solution <PATH>     Path to .sln file (opens solution mode)
  --project <PATH>      Path to .csproj file (opens project mode)
  --help               Show help information
```

### Editor Integration (Zed Example)

Add to your Zed `settings.json`:

```json
{
  "lsp": {
    "roslyn": {
      "binary": {
        "path": "/path/to/roslyn-wrapper",
        "arguments": []
      }
    }
  },
  "languages": {
    "C#": {
      "language_servers": ["roslyn"],
      "format_on_save": "on"
    }
  }
}
```

The wrapper will automatically:
1. Download the appropriate Roslyn language server version on first run
2. Detect and load the solution/project from your workspace root
3. Forward all LSP messages between Zed and Roslyn

## Viewing Roslyn Messages in Zed

roslyn-wrapper is a **transparent proxy** - it doesn't generate custom notifications. All Roslyn error messages, warnings, and progress updates are communicated through standard LSP channels.

### Accessing the LSP Log Viewer

Zed has a built-in LSP Log Viewer that displays all messages from language servers:

1. **Open the viewer:**
   - Command Palette (`Cmd+Shift+P` / `Ctrl+Shift+P`) → "Open Language Server Logs"
   - Or: `View` menu → `Open Language Server Logs`

2. **Filter to roslyn-wrapper:**
   - Select "roslyn-wrapper" from the language server dropdown

3. **Filter by log level:**
   - **Error** (red): Critical errors like missing .NET SDK, project load failures
   - **Warning** (yellow): Warnings during project loading
   - **Info** (blue): Progress updates like "Loading solution...", "Project loaded"
   - **Log** (gray): Verbose diagnostic information

### Common Messages You'll See

#### Project Loading

```
[LanguageServerProjectSystem] Loading /path/to/YourProject.sln...
```

#### SDK Errors

```
Error while loading /path/to/project.csproj:
Microsoft.CodeAnalysis.MSBuild.RemoteInvocationException:
Error while calling hostfxr function hostfxr_resolve_sdk2.
A compatible .NET SDK was not found.
```

**How to fix:** Install the required .NET SDK version specified in your project's `global.json` or the latest .NET SDK.

#### Completion

```
workspace/projectInitializationComplete
```

This notification signals that all projects have finished loading.

## Debugging

### Wrapper Logs

The wrapper writes detailed logs to:

```
Linux/macOS:   ~/.local/share/roslyn-wrapper/roslyn-wrapper.log
Windows:       %LOCALAPPDATA%\roslyn-wrapper\roslyn-wrapper.log
```

These logs include:
- All LSP messages (requests, responses, notifications)
- stderr output from Roslyn process
- Wrapper startup and initialization details

### Viewing Logs

```bash
# Follow the log in real-time
tail -f ~/.local/share/roslyn-wrapper/roslyn-wrapper.log

# Search for errors
grep "Error" ~/.local/share/roslyn-wrapper/roslyn-wrapper.log
```

## Architecture

### Transparent Proxy Design

roslyn-wrapper follows a **transparent proxy** design principle:

```
Editor (Zed) ←→ roslyn-wrapper ←→ Roslyn Language Server
                      ↓
                  log file
```

**What the wrapper does:**
- Forwards all LSP messages unchanged
- Maps non-standard messages to LSP standard equivalents
- Logs all traffic for debugging

**What the wrapper does NOT do:**
- ❌ Generate custom notifications
- ❌ Parse or modify error messages
- ❌ Intercept or suppress toasts/messages
- ❌ Implement custom progress indicators

**Why this design:**
- Roslyn already sends comprehensive error information via LSP standard channels
- Editors like Zed have proper UI for displaying LSP logs
- Custom notifications interfere with Roslyn's error reporting
- Simplicity = fewer bugs and easier maintenance

### Message Flow

1. **Editor → Wrapper → Roslyn:**
   - LSP requests (textDocument/*, workspace/*, etc.)
   - Initialization, configuration, document changes

2. **Roslyn → Wrapper → Editor:**
   - `window/logMessage`: Progress updates, errors, warnings
   - `window/showMessage`: User-facing messages (via `_roslyn_showToast` mapping)
   - `textDocument/publishDiagnostics`: Code errors/warnings
   - `workspace/projectInitializationComplete`: Loading complete

## Troubleshooting

### "A compatible .NET SDK was not found"

**Solution:** Install the .NET SDK version required by your project.

1. Check your project's `global.json` for SDK version requirements
2. Download from: https://dotnet.microsoft.com/download
3. Verify installation: `dotnet --version`
4. Restart the editor

### No Diagnostics/IntelliSense

1. **Check LSP logs** (Command Palette → "Open Language Server Logs")
2. **Look for errors** during project loading
3. **Check wrapper logs** at `~/.local/share/roslyn-wrapper/roslyn-wrapper.log`
4. **Verify .NET SDK** is installed: `dotnet --version`
5. **Restart the language server** (Command Palette → "Restart Language Server")

### Project Not Loading

1. **Ensure a .sln or .csproj file exists** in your workspace root
2. **Check the LSP logs** for error messages
3. **Verify your project builds** with `dotnet build`
4. **Check for missing NuGet packages** - run `dotnet restore`

## Development

### Running Tests

```bash
cargo test
```

### Testing with Large Projects

Use the included `test_progress.py` script to monitor LSP messages:

```bash
python3 test_progress.py /path/to/solution.sln
```

This will:
- Launch roslyn-wrapper with the specified solution
- Send LSP initialization messages
- Monitor all messages for 60 seconds
- Display summary of message types

### Code Structure

```
src/
├── main.rs         # Entry point, LSP proxy logic, message forwarding
├── download.rs     # Roslyn language server download and management
├── logger.rs       # Logging infrastructure
└── path_utils.rs   # Path manipulation utilities
```

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes with clear commit messages
4. Add tests if applicable
5. Submit a pull request

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Related Documentation

- [PROGRESS_FINDINGS.md](PROGRESS_FINDINGS.md) - Investigation into Roslyn's progress notification behavior
- [Roslyn Language Server](https://github.com/dotnet/roslyn/tree/main/src/Features/LanguageServer)
- [LSP Specification](https://microsoft.github.io/language-server-protocol/)

## Acknowledgments

- Built for use with [Zed](https://zed.dev/) editor
- Uses the [Roslyn](https://github.com/dotnet/roslyn) language server from the .NET team
- Inspired by the need for cross-solution C# support in modern editors

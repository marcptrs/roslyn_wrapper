# Roslyn Progress Notification Investigation

## Summary

After testing Roslyn with a large project (Roslyn itself), we discovered that **Roslyn does NOT send LSP progress notifications** (`$/progress` or `window/workDoneProgress/create`). Instead, it uses **`window/logMessage` notifications** to communicate loading progress and errors.

## Test Setup

- **Test script**: `test_progress.py` - Python script that launches roslyn-wrapper and monitors all LSP messages
- **Test project**: Roslyn.sln (large C# solution with many projects)
- **Duration**: 60 seconds of monitoring during project load
- **Messages received**: 307 total LSP messages

## Key Findings

### 1. No Standard Progress Notifications

Roslyn sent **ZERO** of these standard LSP progress messages:
- ❌ `$/progress` notifications
- ❌ `window/workDoneProgress/create` requests

### 2. Roslyn Uses window/logMessage Instead

During project loading, Roslyn sent **~300 `window/logMessage` notifications** with progress information:

#### Message Types (LSP log levels):
- **Type 1 (Error)**: Project loading errors
- **Type 2 (Warning)**: Project load warnings  
- **Type 3 (Info)**: Progress updates
- **Type 4 (Log)**: Verbose logging

#### Example Progress Messages:

**Initial loading:**
```
<<< LOG (3): [Program] Language server initialized
<<< LOG (3): [solution/open] [LanguageServerProjectSystem] Loading /path/to/Roslyn.sln...
```

**Per-project loading errors** (missing SDK in this case):
```
<<< LOG (1): [solution/open] [LanguageServerProjectLoader] Error while loading /path/to/project.csproj: 
Exception thrown: Microsoft.CodeAnalysis.MSBuild.RemoteInvocationException: 
An exception of type System.InvalidOperationException was thrown: 
Error while calling hostfxr function hostfxr_resolve_sdk2. 
Error code: -2147450725 
Detailed error: A compatible .NET SDK was not found.
```

**Completion notification:**
```
<<< NOTIFICATION: workspace/projectInitializationComplete
```

### 3. SDK Errors ARE Communicated

The missing .NET SDK error **IS** communicated to the editor via:
- `window/logMessage` with type=1 (Error)
- Message contains: `"A compatible .NET SDK was not found"`
- Each failing project generates an error message

## Implications for roslyn-wrapper

### Current Implementation Status (v0.2.0)

✅ **Transparent Proxy Design:**
- Wrapper is a **pure transparent proxy** - no custom notifications or message manipulation
- All `window/logMessage` notifications are forwarded unchanged
- All `window/showMessage` requests are forwarded unchanged
- SDK errors are communicated through Roslyn's standard `window/logMessage` (Type 1 - Error)
- `workspace/projectInitializationComplete` is forwarded when loading completes

✅ **Removed Custom Notification Code:**
- ❌ No stderr parsing and custom notification generation
- ❌ No ShowMessageRequest interception/replacement
- ❌ No toast suppression/rewriting logic
- ✅ stderr is only logged to wrapper log file for debugging purposes

### How Users View Roslyn Messages in Zed

**Zed has built-in LSP log viewer** that displays all `window/logMessage` notifications:

1. **Access the LSP Log Viewer:**
   - Command Palette → "Open Language Server Logs" (`lsp: Open Language Server Logs`)
   - Or use `View` menu → `Open Language Server Logs`

2. **View Roslyn Messages:**
   - Select "roslyn-wrapper" from the language server dropdown
   - Filter by log level: Error, Warning, Info, Log
   - All ~300 messages during project loading are captured here

3. **Find SDK Errors:**
   - Look for Error level messages (red)
   - Search for: "A compatible .NET SDK was not found"
   - Each failing project generates a detailed error message

### Wrapper's Role

The wrapper is intentionally minimal and transparent:

1. **Message Forwarding:**
   - ✅ Proxies all LSP messages bidirectionally without modification
   - ✅ Maps Roslyn's custom `window/_roslyn_showToast` → standard `window/showMessage`
   - ✅ Logs all traffic to wrapper log file for debugging

2. **No Custom UI/Notifications:**
   - ❌ Does NOT generate synthetic notifications
   - ❌ Does NOT intercept or modify Roslyn's error messages
   - ❌ Does NOT suppress or replace toasts

3. **Why This Design:**
   - Roslyn already sends comprehensive error information via LSP standard channels
   - Zed has proper UI for displaying LSP logs
   - Custom notifications would interfere with Roslyn's own error reporting
   - Simplicity = fewer bugs and easier maintenance

## Testing Results

### Test Command:
```bash
cd roslyn_wrapper
python3 test_progress.py /path/to/solution.sln
```

### Output:
```
Wrapper: /path/to/roslyn-wrapper
Solution: /path/to/Roslyn.sln
Project root: /path/to/roslyn

Launching wrapper...
Sending initialize request...
>>> SENDING: initialize 501 bytes

Waiting for initialize response...
<<< LOG (3): [Program] Language server initialized
<<< RESPONSE: id=1
<<< INITIALIZE RESPONSE received

Sending initialized notification...
>>> SENDING: initialized 57 bytes

================================================================================
MONITORING FOR PROGRESS NOTIFICATIONS (60 seconds)...
================================================================================

<<< LOG (3): [solution/open] [LanguageServerProjectSystem] Loading /path/to/Roslyn.sln...
<<< NOTIFICATION: workspace/configuration
<<< LOG (1): [solution/open] [LanguageServerProjectLoader] Error while loading ...
... (300+ more log messages) ...
<<< NOTIFICATION: workspace/projectInitializationComplete

================================================================================
SUMMARY
================================================================================
Total messages received: 307
Progress-related messages: 0

⚠️  NO PROGRESS MESSAGES DETECTED
   This means Roslyn is not sending $/progress or window/workDoneProgress/create
```

## Message Patterns to Watch For

If implementing custom progress indicators based on log messages:

### Loading Start:
- Pattern: `[LanguageServerProjectSystem] Loading .*.sln`
- Type: Info (3)
- Action: Show "Loading project..." indicator

### Per-Project Status:
- Pattern: `[LanguageServerProjectLoader] (Error|Unable to load) project`
- Type: Error (1) or Warning (2)
- Action: Count failures, show in status

### SDK Missing:
- Pattern: `A compatible .NET SDK was not found`
- Type: Error (1)
- Action: Show prominent user notification with setup instructions

### Loading Complete:
- Method: `workspace/projectInitializationComplete`
- Action: Hide loading indicator, show ready status

## Conclusion

The roslyn-wrapper is a **transparent LSP proxy** that:

1. ✅ Forwards all Roslyn messages unchanged (including all `window/logMessage` with progress/errors)
2. ✅ Maps non-standard `window/_roslyn_showToast` → standard `window/showMessage`
3. ✅ Forwards `workspace/projectInitializationComplete` completion notification
4. ✅ Logs all traffic to file for debugging

**No custom notifications are generated** because:
- Roslyn already sends rich progress information via `window/logMessage`
- SDK errors are already communicated through these log messages  
- Zed has a built-in LSP Log Viewer to display all messages
- Custom notifications would interfere with Roslyn's error reporting

**User experience:**
- Users view all Roslyn messages via: Command Palette → "Open Language Server Logs"
- Filter by server: "roslyn-wrapper"
- Filter by level: Error, Warning, Info, Log
- SDK errors appear as Error level messages with full details

The wrapper's philosophy: **Be transparent, forward faithfully, let the editor handle UI.**

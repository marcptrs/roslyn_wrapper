use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::path::PathBuf;

use serde_json::{json, Value};

mod download;
mod logger;
mod path_utils;

// LSP Message Type Constants (for window/showMessage)
const LSP_MESSAGE_TYPE_ERROR: i64 = 1;
const LSP_MESSAGE_TYPE_WARNING: i64 = 2;
const LSP_MESSAGE_TYPE_INFO: i64 = 3;

// Roslyn Message Type Constants (for window/_roslyn_showToast)
const ROSLYN_MESSAGE_TYPE_ERROR: i64 = 3;
const ROSLYN_MESSAGE_TYPE_WARNING: i64 = 1;
const ROSLYN_MESSAGE_TYPE_INFO: i64 = 2;

/// LSP Message Wrapper for Roslyn
/// 
/// This wrapper acts as a proxy between Zed and the Roslyn Language Server.
/// Key responsibilities:
/// 1. Start Roslyn subprocess
/// 2. Forward LSP messages bidirectionally using async tasks
/// 3. Inject `solution/open` notification after initialization
/// 4. Handle edge cases and logging

/// Parse LSP message header and body from a reader
fn read_lsp_message<R: Read + BufRead>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut content_length = 0;
    let mut line = String::new();

    // Read headers until empty line
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None); // EOF
        }

        let line_trimmed = line.trim();
        if line_trimmed.is_empty() {
            break;
        }

        if line_trimmed.starts_with("Content-Length:") {
            let parts: Vec<&str> = line_trimmed.split(':').collect();
            if let Some(len_str) = parts.get(1) {
                content_length = len_str.trim().parse().unwrap_or(0);
            }
        }
    }

    if content_length == 0 {
        return Ok(None);
    }

    // Read body
    let mut buf = vec![0; content_length];
    reader.read_exact(&mut buf)?;

    let body = String::from_utf8_lossy(&buf);
    match serde_json::from_str::<Value>(&body) {
        Ok(value) => Ok(Some(value)),
        Err(e) => {
            logger::error(format!("[roslyn_wrapper] Failed to parse LSP message: {}", e));
            Ok(None)
        }
    }
}

/// Send an LSP message to a writer
fn send_lsp_message<W: Write>(writer: &mut W, msg: &Value) -> io::Result<()> {
    let json_str = msg.to_string();
    let header = format!("Content-Length: {}\r\n\r\n", json_str.len());
    
    writer.write_all(header.as_bytes())?;
    writer.write_all(json_str.as_bytes())?;
    writer.flush()?;

    Ok(())
}

fn main() -> io::Result<()> {
    // Use tokio runtime to run async code
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        run().await
    })
}

/// Handle pass-through mode for Roslyn arguments (--version, --help, etc.)
async fn handle_passthrough_mode(args: &[String]) -> io::Result<()> {
    logger::info("[roslyn_wrapper] Pass-through mode: forwarding arguments to Roslyn");
    
    // Download/find Roslyn first
    let roslyn_path = download::get_roslyn_path()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    
    // Execute Roslyn with the provided arguments
    let status = Command::new(roslyn_path)
        .args(&args[1..])
        .status()?;
    
    std::process::exit(status.code().unwrap_or(1));
}

/// Resolve the Roslyn LSP binary path from arguments or download
async fn get_roslyn_lsp_path(args: &[String]) -> io::Result<String> {
    if let Some(path_arg) = args.get(1) {
        logger::info(format!("[roslyn_wrapper] Using Roslyn LSP path from extension: {}", path_arg));
        
        // Normalize path
        #[cfg(windows)]
        let normalized = path_arg.replace('/', "\\");
        #[cfg(not(windows))]
        let normalized = path_arg.clone();
        
        // Verify file exists
        let path_to_use = match std::fs::metadata(&normalized) {
            Ok(_) => normalized,
            Err(_) => {
                match std::fs::metadata(&path_arg) {
                    Ok(_) => path_arg.to_string(),
                    Err(_) => {
                        logger::error(format!("[roslyn_wrapper] Cannot find Roslyn LSP at: {}", path_arg));
                        return Err(io::Error::new(io::ErrorKind::NotFound, 
                            format!("Cannot find Roslyn LSP at: {}", path_arg)));
                    }
                }
            }
        };
        
        Ok(path_to_use)
    } else {
        logger::info("[roslyn_wrapper] No Roslyn LSP path provided, attempting to download...");
        let roslyn_path = download::get_roslyn_path()
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        
        Ok(roslyn_path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid Roslyn path"))?
            .to_string())
    }
}

async fn run() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    // Check if we should pass through arguments to Roslyn (e.g., --version, --help)
    if args.len() > 1 {
        let first_arg = &args[1];
        
        // If first argument looks like a flag (starts with -), pass through to Roslyn
        if first_arg.starts_with('-') {
            return handle_passthrough_mode(&args).await;
        }
    }
    
    // LSP proxy mode: Get Roslyn LSP path from command-line arguments or download
    let roslyn_path_str = get_roslyn_lsp_path(&args).await?;

    logger::info(format!("[roslyn_wrapper] Starting Roslyn process: {}", roslyn_path_str));
    
    // Start Roslyn subprocess
    let mut roslyn_process = Command::new(&roslyn_path_str)
        .args(&["--extensionLogDirectory", ".", "--logLevel", "Information", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            logger::error(format!("[roslyn_wrapper] Failed to spawn Roslyn: {}", e));
            e
        })?;

    let roslyn_stdin = roslyn_process
        .stdin
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to get Roslyn stdin"))?;

    let roslyn_stdout = roslyn_process
        .stdout
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to get Roslyn stdout"))?;
    let roslyn_stderr = roslyn_process
        .stderr
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to get Roslyn stderr"))?;

    logger::info("[roslyn_wrapper] Roslyn process started successfully");
    
    // Wrap in Arc<Mutex<>> for sharing between tasks
    let roslyn_stdin = Arc::new(Mutex::new(roslyn_stdin));
    let mut roslyn_stdout = BufReader::new(roslyn_stdout);
    
    // Create stdout early so it can be cloned for stderr task
    let stdin = io::stdin();
    let mut stdin = BufReader::new(stdin);
    let stdout = Arc::new(Mutex::new(io::stdout()));

    // Pipe Roslyn stderr to wrapper logs for debugging
    let mut roslyn_stderr_reader = BufReader::new(roslyn_stderr);
    let _stderr_task = tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match roslyn_stderr_reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let msg = line.trim_end();
                    if !msg.is_empty() {
                        logger::debug(format!("[roslyn][stderr] {}", msg));
                    }
                }
                Err(_) => break,
            }
        }
    });
    
    // Shared state for initialization
    let initialized = Arc::new(Mutex::new(false));
    let solution_uri: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let workspace_roots: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));

    // Track request IDs to methods to normalize responses when needed
    let id_method_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    
    logger::debug("[roslyn_wrapper] Starting bidirectional message forwarding");

    // Spawn task to forward messages from client to Roslyn
    let roslyn_stdin_clone = Arc::clone(&roslyn_stdin);
    let solution_uri_clone = Arc::clone(&solution_uri);
    let workspace_roots_c2r = Arc::clone(&workspace_roots);
    let id_method_map_c2r = Arc::clone(&id_method_map);
    
    let client_to_roslyn = tokio::task::spawn_blocking(move || {
        loop {
            match read_lsp_message(&mut stdin) {
                Ok(Some(msg)) => {
                    logger::debug(format!("[roslyn_wrapper] <== FROM CLIENT"));
                    
                    // Record request method by id for response normalization
                    if let (Some(id_val), Some(method)) = (msg.get("id"), msg.get("method").and_then(|v| v.as_str())) {
                        // Only track a few methods we may normalize
                        let should_track = matches!(method, "textDocument/diagnostic");
                        if should_track {
                            let mut map = id_method_map_c2r.blocking_lock();
                            map.insert(id_val.to_string(), method.to_string());
                        }
                    }

                    // Check for initialize request to extract solution URI
                    if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
                        if method == "initialize" {
                            if let Some(params) = msg.get("params") {
                                // capture workspace rootUri if present
                                if let Some(root_uri) = params.get("rootUri").and_then(|v| v.as_str()) {
                                    if let Ok(path) = path_utils::url_to_path(root_uri) {
                                        let mut roots = workspace_roots_c2r.blocking_lock();
                                        roots.clear();
                                        roots.push(path);
                                        logger::info("[roslyn_wrapper] Captured workspace rootUri");
                                    }
                                }
                                // capture workspaceFolders if present
                                if let Some(folders) = params.get("workspaceFolders").and_then(|v| v.as_array()) {
                                    let mut roots = workspace_roots_c2r.blocking_lock();
                                    if roots.is_empty() {
                                        for f in folders {
                                            if let Some(uri) = f.get("uri").and_then(|u| u.as_str()) {
                                                if let Ok(p) = path_utils::url_to_path(uri) {
                                                    roots.push(p);
                                                }
                                            }
                                        }
                                        if !roots.is_empty() {
                                            logger::info("[roslyn_wrapper] Captured workspaceFolders");
                                        }
                                    }
                                }
                                if let Some(init_opts) = params.get("initializationOptions") {
                                    if let Some(solution) = init_opts.get("solution").and_then(|v| v.as_str()) {
                                        let mut sol_uri = solution_uri_clone.blocking_lock();
                                        *sol_uri = Some(solution.to_string());
                                        logger::info("[roslyn_wrapper] Found solution URI");
                                    }
                                }
                            }
                        }
                    }
                    
                    // Forward to Roslyn
                    let mut roslyn_stdin = roslyn_stdin_clone.blocking_lock();
                    if let Err(e) = send_lsp_message(&mut *roslyn_stdin, &msg) {
                        logger::error(format!("[roslyn_wrapper] Error forwarding to Roslyn: {}", e));
                        break;
                    }
                    logger::debug("[roslyn_wrapper] ==> TO ROSLYN");
                }
                Ok(None) => {
                    logger::info("[roslyn_wrapper] Client closed connection");
                    break;
                }
                Err(e) => {
                    logger::error(format!("[roslyn_wrapper] Error reading from client: {}", e));
                    break;
                }
            }
        }
    });

    // Main task: forward messages from Roslyn to client
    let id_method_map_r2c = Arc::clone(&id_method_map);
    let workspace_roots_r2c = Arc::clone(&workspace_roots);
    let stdout_r2c = Arc::clone(&stdout);
    let roslyn_to_client = tokio::task::spawn_blocking(move || {
        loop {
            match read_lsp_message(&mut roslyn_stdout) {
                Ok(Some(mut msg)) => {
                    logger::debug("[roslyn_wrapper] <== FROM ROSLYN");
                    
                    // Normalize certain server->client requests with unit params
                    let method_opt = msg.get("method").and_then(|v| v.as_str()).map(|s| s.to_string());
                    if let Some(method) = method_opt {
                        if matches!(method.as_str(),
                            "workspace/inlayHint/refresh" |
                            "workspace/diagnostic/refresh" |
                            "workspace/codeLens/refresh"
                        ) {
                            let needs_fix = match msg.get("params") {
                                None => true,
                                Some(v) if !v.is_object() => true, // [] or null → {}
                                _ => false,
                            };
                            if needs_fix {
                                if let Some(obj) = msg.as_object_mut() {
                                    obj.remove("params");
                                    logger::debug(format!("[roslyn_wrapper] Removed params for unit method {}", method));
                                }
                            }
                        }
                    }
                    
                    // Check if this is initialization response
                    if let Some(result) = msg.get("result") {
                        if result.get("capabilities").is_some() {
                            let mut init = initialized.blocking_lock();
                            if !*init {
                                *init = true;
                                logger::info("[roslyn_wrapper] Initialization complete");
                                
                                // Forward response to client first
                                let mut stdout_lock = stdout.blocking_lock();
                                if let Err(e) = send_lsp_message(&mut *stdout_lock, &msg) {
                                     logger::error(format!("[roslyn_wrapper] Error forwarding to client: {}", e));
                                    break;
                                }
                                logger::debug("[roslyn_wrapper] ==> TO CLIENT");
                                
                                drop(stdout_lock); // Release lock
                                
                                // Then send solution/open notification
                                let sol_uri = solution_uri.blocking_lock();
                                let maybe_solution = if sol_uri.is_some() {
                                    sol_uri.clone()
                                } else {
                                    // attempt discovery from all workspace roots (rootUri and workspaceFolders)
                                    let roots = workspace_roots_r2c.blocking_lock();
                                    let mut found: Option<String> = None;
                                    for r in roots.iter() {
                                        if let Some(uri) = path_utils::try_find_solution_or_project(r) {
                                            found = Some(uri);
                                            break;
                                        }
                                    }
                                    found
                                };
                                if let Some(uri) = maybe_solution {
                                    let notification = json!({
                                        "jsonrpc": "2.0",
                                        "method": "solution/open",
                                        "params": {
                                            "solution": uri
                                        }
                                    });
                                    logger::info("[roslyn_wrapper] Sending solution/open notification");
                                    let mut roslyn_stdin = roslyn_stdin.blocking_lock();
                                    if let Err(e) = send_lsp_message(&mut *roslyn_stdin, &notification) {
                                        logger::error(format!("[roslyn_wrapper] Error sending solution/open: {}", e));
                                    }
                                } else {
                                    logger::info("[roslyn_wrapper] No solution or project found to open");
                                    // Inform the client so users understand why features are limited
                                    let info_msg = json!({
                                        "jsonrpc": "2.0",
                                        "method": "window/showMessage",
                                        "params": {
                                            "type": LSP_MESSAGE_TYPE_WARNING,
                                            "message": "No .sln or .csproj found in the workspace. C# features are limited until a solution or project is opened. Open a folder with a .sln/.csproj or configure the 'solution' option in the C# extension."
                                        }
                                    });
                                    let mut stdout_lock = stdout.blocking_lock();
                                    if let Err(e) = send_lsp_message(&mut *stdout_lock, &info_msg) {
                                        logger::error(format!("[roslyn_wrapper] Failed to send no-solution warning: {}", e));
                                    }
                                }
                                
                                continue; // Already forwarded, skip duplicate
                            }
                        }
                    }

                    // Normalize null results for known requests (e.g., textDocument/diagnostic)
                    if let Some(id_val) = msg.get("id") {
                        let id_key = id_val.to_string();
                        let tracked_method = {
                            let mut map = id_method_map_r2c.blocking_lock();
                            map.remove(&id_key)
                        };
                        if let Some(method) = tracked_method {
                            if method == "textDocument/diagnostic" {
                                let need_fix = match msg.get("result") {
                                    None => true,
                                    Some(v) if v.is_null() => true,
                                    _ => false,
                                };
                                if need_fix {
                                    if let Some(obj) = msg.as_object_mut() {
                                        obj.insert("result".to_string(), json!({
                                            "kind": "full",
                                            "items": []
                                        }));
                                        logger::debug("[roslyn_wrapper] Normalized null diagnostic result to empty report");
                                    }
                                }
                            }
                        }
                    }
                    
                    // Map Roslyn custom toast notifications to standard LSP showMessage
                    let forward_msg = if let Some(method_name) = msg.get("method").and_then(|v| v.as_str()) {
                        if method_name == "window/_roslyn_showToast" {
                            if let Some(params) = msg.get("params") {
                                let message = params.get("message").and_then(|v| v.as_str()).unwrap_or("");
                                let roslyn_type = params.get("messageType").and_then(|v| v.as_i64()).unwrap_or(ROSLYN_MESSAGE_TYPE_INFO);
                                // Map Roslyn message types to LSP: 3->1 (Error), 1->2 (Warning), 2->3 (Info)
                                let lsp_type = match roslyn_type {
                                    ROSLYN_MESSAGE_TYPE_ERROR => LSP_MESSAGE_TYPE_ERROR,
                                    ROSLYN_MESSAGE_TYPE_WARNING => LSP_MESSAGE_TYPE_WARNING,
                                    ROSLYN_MESSAGE_TYPE_INFO => LSP_MESSAGE_TYPE_INFO,
                                    _ => LSP_MESSAGE_TYPE_INFO,
                                };
                                
                                logger::debug(format!("[roslyn_wrapper] Rewriting _roslyn_showToast to window/showMessage"));
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "window/showMessage",
                                    "params": {
                                        "type": lsp_type,
                                        "message": message
                                    }
                                })
                            } else {
                                msg
                            }
                        } else {
                            msg
                        }
                    } else {
                        msg
                    };

                    // Forward to client
                    let mut stdout = stdout_r2c.blocking_lock();
                    if let Err(e) = send_lsp_message(&mut *stdout, &forward_msg) {
                        logger::error(format!("[roslyn_wrapper] Error forwarding to client: {}", e));
                        break;
                    }
                    logger::debug("[roslyn_wrapper] ==> TO CLIENT");
                }
                Ok(None) => {
                    logger::info("[roslyn_wrapper] Roslyn closed connection");
                    break;
                }
                Err(e) => {
                    logger::error(format!("[roslyn_wrapper] Error reading from Roslyn: {}", e));
                    break;
                }
            }
        }
    });

    // Wait for either task to complete (which means connection closed)
    tokio::select! {
        _ = client_to_roslyn => {
            logger::debug("[roslyn_wrapper] Client to Roslyn task completed");
        }
        _ = roslyn_to_client => {
            logger::debug("[roslyn_wrapper] Roslyn to Client task completed");
        }
    }

    logger::info("[roslyn_wrapper] Shutting down");
    Ok(())
}

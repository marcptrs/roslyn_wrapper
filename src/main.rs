use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::sync::Mutex;

use serde_json::{json, Value};

mod download;
mod logger;

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

async fn run() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    // Check if we should pass through arguments to Roslyn (e.g., --version, --help)
    if args.len() > 1 {
        let first_arg = &args[1];
        
        // If first argument looks like a flag (starts with -), pass through to Roslyn
        if first_arg.starts_with('-') {
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
    }
    
    // LSP proxy mode: Get Roslyn LSP path from command-line arguments or download
    let roslyn_path_str = if let Some(path_arg) = args.get(1) {
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
        
        path_to_use
    } else {
        logger::info("[roslyn_wrapper] No Roslyn LSP path provided, attempting to download...");
        let roslyn_path = download::get_roslyn_path()
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        
        roslyn_path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid Roslyn path"))?
            .to_string()
    };

    logger::info(format!("[roslyn_wrapper] Starting Roslyn process: {}", roslyn_path_str));
    
    // Start Roslyn subprocess
    let mut roslyn_process = Command::new(&roslyn_path_str)
        .args(&["--extensionLogDirectory", ".", "--logLevel", "Information", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
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

    logger::info("[roslyn_wrapper] Roslyn process started successfully");
    
    // Wrap in Arc<Mutex<>> for sharing between tasks
    let roslyn_stdin = Arc::new(Mutex::new(roslyn_stdin));
    let mut roslyn_stdout = BufReader::new(roslyn_stdout);
    
    let stdin = io::stdin();
    let mut stdin = BufReader::new(stdin);
    let stdout = Arc::new(Mutex::new(io::stdout()));
    
    // Shared state for initialization
    let initialized = Arc::new(Mutex::new(false));
    let solution_uri: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    
    logger::debug("[roslyn_wrapper] Starting bidirectional message forwarding");

    // Spawn task to forward messages from client to Roslyn
    let roslyn_stdin_clone = Arc::clone(&roslyn_stdin);
    let solution_uri_clone = Arc::clone(&solution_uri);
    // let initialized_clone = Arc::clone(&initialized);
    
    let client_to_roslyn = tokio::task::spawn_blocking(move || {
        loop {
            match read_lsp_message(&mut stdin) {
                Ok(Some(msg)) => {
                    logger::debug(format!("[roslyn_wrapper] <== FROM CLIENT"));
                    
                    // Check for initialize request to extract solution URI
                    if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
                        if method == "initialize" {
                            if let Some(params) = msg.get("params") {
                                if let Some(init_opts) = params.get("initializationOptions") {
                                    if let Some(solution) = init_opts.get("solution").and_then(|v| v.as_str()) {
                                        let mut sol_uri = solution_uri_clone.blocking_lock();
                                        *sol_uri = Some(solution.to_string());
                                        logger::info(format!("[roslyn_wrapper] Found solution URI"));
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
    let roslyn_to_client = tokio::task::spawn_blocking(move || {
        loop {
            match read_lsp_message(&mut roslyn_stdout) {
                Ok(Some(msg)) => {
                    logger::debug("[roslyn_wrapper] <== FROM ROSLYN");
                    
                    // Check if this is initialization response
                    if let Some(result) = msg.get("result") {
                        if result.get("capabilities").is_some() {
                            let mut init = initialized.blocking_lock();
                            if !*init {
                                *init = true;
                                logger::info("[roslyn_wrapper] Initialization complete");
                                
                                // Forward response to client first
                                let mut stdout = stdout.blocking_lock();
                                if let Err(e) = send_lsp_message(&mut *stdout, &msg) {
                                     logger::error(format!("[roslyn_wrapper] Error forwarding to client: {}", e));
                                    break;
                                }
                                logger::debug("[roslyn_wrapper] ==> TO CLIENT");
                                
                                drop(stdout); // Release lock
                                
                                // Then send solution/open notification
                                let sol_uri = solution_uri.blocking_lock();
                                if let Some(ref uri) = *sol_uri {
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
                                }
                                
                                continue; // Already forwarded, skip duplicate
                            }
                        }
                    }
                    
                    // Forward to client
                    let mut stdout = stdout.blocking_lock();
                    if let Err(e) = send_lsp_message(&mut *stdout, &msg) {
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

use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use serde_json::{json, Value};

mod download;

/// LSP Message Wrapper for Roslyn
/// 
/// This wrapper acts as a proxy between Zed and the Roslyn Language Server.
/// Key responsibilities:
/// 1. Start Roslyn subprocess
/// 2. Forward LSP messages bidirectionally
/// 3. Inject `solution/open` notification after initialization
/// 4. Handle edge cases and logging

struct LspProxy {
    roslyn_stdin: std::process::ChildStdin,
    roslyn_stdout: BufReader<std::process::ChildStdout>,
}

impl LspProxy {
    /// Start the Roslyn language server and create a proxy
    fn start(roslyn_path: &str) -> io::Result<Self> {
        eprintln!("[roslyn_wrapper] Attempting to spawn Roslyn process from: {}", roslyn_path);
        
        // Check if file exists
        match std::fs::metadata(roslyn_path) {
            Ok(metadata) => {
                eprintln!("[roslyn_wrapper] File exists. Size: {} bytes, is_file: {}", metadata.len(), metadata.is_file());
            }
            Err(e) => {
                eprintln!("[roslyn_wrapper] File check failed: {}", e);
            }
        }
        
        let mut roslyn_process = Command::new(roslyn_path)
            .args(&["--extensionLogDirectory", ".", "--logLevel", "Information", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                eprintln!("[roslyn_wrapper] Failed to spawn process: {}", e);
                e
            })?;

        let roslyn_stdin = roslyn_process
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to get stdin"))?;

        let roslyn_stdout = roslyn_process
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Failed to get stdout"))?;

        let roslyn_stdout = BufReader::new(roslyn_stdout);

        Ok(LspProxy {
            roslyn_stdin,
            roslyn_stdout,
        })
    }

    /// Parse LSP message header and body
    fn read_message(&mut self) -> io::Result<Option<Value>> {
        let mut content_length = 0;
        let mut line = String::new();

        // Read headers until empty line
        loop {
            line.clear();
            let n = self.roslyn_stdout.read_line(&mut line)?;
            if n == 0 {
                return Ok(None); // EOF
            }

            let line = line.trim();
            if line.is_empty() {
                break;
            }

            if line.starts_with("Content-Length:") {
                let parts: Vec<&str> = line.split(':').collect();
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
        self.roslyn_stdout.read_exact(&mut buf)?;

        let body = String::from_utf8_lossy(&buf);
        match serde_json::from_str::<Value>(&body) {
            Ok(value) => Ok(Some(value)),
            Err(e) => {
                eprintln!("[roslyn_wrapper] Failed to parse LSP message: {}", e);
                Ok(None)
            }
        }
    }

    /// Send an LSP message
    fn send_message(&mut self, msg: &Value) -> io::Result<()> {
        let json_str = msg.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", json_str.len());
        
        self.roslyn_stdin.write_all(header.as_bytes())?;
        self.roslyn_stdin.write_all(json_str.as_bytes())?;
        self.roslyn_stdin.flush()?;

        Ok(())
    }

    /// Forward a message from client to Roslyn
    fn forward_to_roslyn(&mut self, msg: &Value) -> io::Result<()> {
        self.send_message(msg)
    }

    /// Forward a message from Roslyn to client
    fn forward_to_client(stdout: &mut std::io::Stdout, msg: &Value) -> io::Result<()> {
        let json_str = msg.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", json_str.len());
        
        stdout.write_all(header.as_bytes())?;
        stdout.write_all(json_str.as_bytes())?;
        stdout.flush()?;

        Ok(())
    }
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
            eprintln!("[roslyn_wrapper] Pass-through mode: forwarding arguments to Roslyn");
            
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
        eprintln!("[roslyn_wrapper] Using Roslyn LSP path from extension: {}", path_arg);
        eprintln!("[roslyn_wrapper] Path length: {} chars", path_arg.len());
        
        // Normalize path: handle both forward and backward slashes
        // Windows can handle forward slashes, but we normalize to backslashes for consistency
        #[cfg(windows)]
        let normalized = path_arg.replace('/', "\\");
        #[cfg(not(windows))]
        let normalized = path_arg.clone();
        
        eprintln!("[roslyn_wrapper] Normalized Roslyn LSP path: {}", normalized);
        
        // Verify file exists - try normalized first, then original
        let path_to_use = match std::fs::metadata(&normalized) {
            Ok(metadata) => {
                eprintln!("[roslyn_wrapper] File exists at normalized path. Size: {} bytes", metadata.len());
                normalized
            }
            Err(e) => {
                eprintln!("[roslyn_wrapper] ERROR: File not found at normalized path: {}", e);
                // Try the original path format as well
                match std::fs::metadata(&path_arg) {
                    Ok(metadata) => {
                        eprintln!("[roslyn_wrapper] Original path format works! Size: {} bytes", metadata.len());
                        path_arg.to_string()
                    }
                    Err(e2) => {
                        eprintln!("[roslyn_wrapper] ERROR: Original path also failed: {}", e2);
                        return Err(io::Error::new(io::ErrorKind::NotFound, 
                            format!("Cannot find Roslyn LSP at: {} or {}", normalized, path_arg)));
                    }
                }
            }
        };
        
        path_to_use
    } else {
        eprintln!("[roslyn_wrapper] No Roslyn LSP path provided, attempting to download...");
        let roslyn_path = download::get_roslyn_path()
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        
        roslyn_path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid Roslyn path"))?
            .to_string()
    };

    let mut proxy = LspProxy::start(&roslyn_path_str)?;
    let mut stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();

    let mut initialized = false;
    let mut solution_uri: Option<String> = None;

    eprintln!("[roslyn_wrapper] Entering main LSP message loop...");

    loop {
        // Read message from client (Zed)
        eprintln!("[roslyn_wrapper] Waiting for client message...");
        let mut content_length = 0;
        let mut line = String::new();

        // Read headers
        loop {
            line.clear();
            let n = stdin.read_line(&mut line)?;
            if n == 0 {
                return Ok(()); // EOF - client closed connection
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }

            if trimmed.starts_with("Content-Length:") {
                let parts: Vec<&str> = trimmed.split(':').collect();
                if let Some(len_str) = parts.get(1) {
                    content_length = len_str.trim().parse().unwrap_or(0);
                }
            }
        }

        if content_length == 0 {
            continue;
        }

        // Read body
        let mut buf = vec![0; content_length];
        stdin.read_exact(&mut buf)?;
        let body = String::from_utf8_lossy(&buf);

        // Parse client message
        if let Ok(client_msg) = serde_json::from_str::<Value>(&body) {
            eprintln!("[roslyn_wrapper] <== FROM CLIENT: {}", serde_json::to_string(&client_msg).unwrap_or_else(|_| "invalid".to_string()));
            
            // Check for initialization request to extract solution URI
            if let Some(method) = client_msg.get("method").and_then(|v| v.as_str()) {
                if method == "initialize" {
                    if let Some(params) = client_msg.get("params") {
                        if let Some(init_opts) = params.get("initializationOptions") {
                            if let Some(solution) = init_opts.get("solution").and_then(|v| v.as_str()) {
                                solution_uri = Some(solution.to_string());
                                eprintln!("[roslyn_wrapper] Found solution URI: {}", solution);
                            }
                        }
                    }
                }
            }

            // Forward client message to Roslyn
            eprintln!("[roslyn_wrapper] ==> TO ROSLYN: {}", serde_json::to_string(&client_msg).unwrap_or_else(|_| "invalid".to_string()));
            proxy.forward_to_roslyn(&client_msg)?;

            // Get the request ID if this is a request (not a notification)
            let request_id = client_msg.get("id").cloned();

            // Read all messages from Roslyn until we get the response (if this was a request)
            // Roslyn may send notifications before the actual response
            loop {
                match proxy.read_message() {
                    Ok(Some(roslyn_msg)) => {
                        eprintln!("[roslyn_wrapper] <== FROM ROSLYN: {}", serde_json::to_string(&roslyn_msg).unwrap_or_else(|_| "invalid".to_string()));
                        
                        // Check if this is a response (has an id field matching our request)
                        let is_response = if let Some(ref req_id) = request_id {
                            roslyn_msg.get("id") == Some(req_id)
                        } else {
                            false
                        };

                        // Check if this is initialization response
                        if is_response {
                            if let Some(result) = roslyn_msg.get("result") {
                                if result.get("capabilities").is_some() {
                                    initialized = true;
                                    eprintln!("[roslyn_wrapper] Initialization complete");

                                    // Forward response to client first
                                    eprintln!("[roslyn_wrapper] ==> TO CLIENT (init response): {}", serde_json::to_string(&roslyn_msg).unwrap_or_else(|_| "invalid".to_string()));
                                    LspProxy::forward_to_client(&mut stdout, &roslyn_msg)?;

                                    // Then send solution/open notification if we have a solution
                                    if let Some(ref sol_uri) = solution_uri {
                                        let notification = json!({
                                            "jsonrpc": "2.0",
                                            "method": "solution/open",
                                            "params": {
                                                "solution": sol_uri
                                            }
                                        });

                                        eprintln!("[roslyn_wrapper] Sending solution/open notification");
                                        if let Err(e) = proxy.send_message(&notification) {
                                            eprintln!("[roslyn_wrapper] Error sending solution/open: {}", e);
                                        }
                                    }

                                    break; // Done processing this request
                                }
                            }
                            
                            // Forward any other response to client
                            eprintln!("[roslyn_wrapper] ==> TO CLIENT (response): {}", serde_json::to_string(&roslyn_msg).unwrap_or_else(|_| "invalid".to_string()));
                            LspProxy::forward_to_client(&mut stdout, &roslyn_msg)?;
                            break; // Done processing this request
                        } else {
                            // This is a notification or other message, forward it immediately
                            eprintln!("[roslyn_wrapper] ==> TO CLIENT (notification): {}", serde_json::to_string(&roslyn_msg).unwrap_or_else(|_| "invalid".to_string()));
                            LspProxy::forward_to_client(&mut stdout, &roslyn_msg)?;
                            
                            // If the original client message was a notification (no id), stop here
                            if request_id.is_none() {
                                break;
                            }
                            // Otherwise, continue reading to get the response
                        }
                    }
                    Ok(None) => {
                        eprintln!("[roslyn_wrapper] Roslyn closed connection");
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("[roslyn_wrapper] Error reading from Roslyn: {}", e);
                        return Err(e);
                    }
                }
            }
        }

        // Handle ongoing message forwarding after initialization
        if initialized {
            // Keep forwarding messages between client and Roslyn
            loop {
                match proxy.read_message() {
                    Ok(Some(roslyn_msg)) => {
                        if let Err(e) = LspProxy::forward_to_client(&mut stdout, &roslyn_msg) {
                            eprintln!("[roslyn_wrapper] Error forwarding to client: {}", e);
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("[roslyn_wrapper] Error reading from Roslyn: {}", e);
                        break;
                    }
                }
            }
        }
    }
}

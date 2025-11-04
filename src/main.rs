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
    // Get Roslyn LSP path from command-line arguments (provided by csharp_roslyn extension)
    let roslyn_path_str = if let Some(path_arg) = std::env::args().nth(1) {
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
                        path_arg
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

    loop {
        // Read message from client (Zed)
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
            proxy.forward_to_roslyn(&client_msg)?;

            // Read response from Roslyn
            if let Ok(Some(roslyn_msg)) = proxy.read_message() {
                // Check if this is initialization response
                if roslyn_msg.get("method").is_none() && roslyn_msg.get("result").is_some() {
                    // This is a response (could be to the initialize request)
                    if let Some(result) = roslyn_msg.get("result") {
                        if result.get("capabilities").is_some() {
                            initialized = true;
                            eprintln!("[roslyn_wrapper] Initialization complete");

                            // Forward response to client first
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

                            continue; // Skip the forward below since we handled it
                        }
                    }
                }

                // Forward response to client
                LspProxy::forward_to_client(&mut stdout, &roslyn_msg)?;
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

use anyhow::{Context, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[derive(Parser, Debug)]
#[command(name = "roslyn-wrapper")]
#[command(about = "A transparent LSP wrapper for Roslyn C# language server", long_about = None)]
struct Args {
    /// Path to Roslyn server DLL
    #[arg(required = true)]
    server_path: String,

    /// Enable debug logging to logs/proxy-debug.log
    #[arg(long, default_value_t = false)]
    debug: bool,

    /// Additional arguments to pass to the server
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    server_args: Vec<String>,
}

struct Logger {
    file: Option<Arc<Mutex<std::fs::File>>>,
    enabled: bool,
}

impl Logger {
    fn new(enabled: bool) -> Self {
        let file = if enabled {
            let log_dir = std::path::Path::new("logs");
            std::fs::create_dir_all(&log_dir).ok();

            OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_dir.join("proxy-debug.log"))
                .ok()
                .map(|f| Arc::new(Mutex::new(f)))
        } else {
            None
        };

        Logger { file, enabled }
    }

    fn log(&self, message: &str) {
        if !self.enabled {
            return;
        }

        if let Some(file) = &self.file {
            if let Ok(mut file) = file.lock() {
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                writeln!(file, "[{}] {}", timestamp, message).ok();
            }
        }
    }
}

impl Clone for Logger {
    fn clone(&self) -> Self {
        Logger {
            file: self.file.clone(),
            enabled: self.enabled,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let logger = Logger::new(args.debug);

    logger.log("Starting Roslyn LSP wrapper");
    logger.log(&format!("Server DLL: {}", args.server_path));
    logger.log(&format!("Debug logging: {}", args.debug));
    logger.log(&format!("Additional args: {:?}", args.server_args));

    let dotnet_path = find_dotnet().context("Failed to find dotnet executable")?;
    logger.log(&format!("Using dotnet at: {}", dotnet_path));

    let log_dir = std::path::Path::new("logs");
    std::fs::create_dir_all(&log_dir).ok();
    let log_dir_str = log_dir.to_string_lossy().to_string();

    let mut command_args = vec![
        args.server_path.clone(),
        "--stdio".to_string(),
        "--logLevel".to_string(),
        "Information".to_string(),
        "--extensionLogDirectory".to_string(),
        log_dir_str,
    ];
    command_args.extend(args.server_args.iter().cloned());

    logger.log(&format!(
        "Spawning: {} {}",
        dotnet_path,
        command_args.join(" ")
    ));

    let mut server_process = Command::new(&dotnet_path)
        .args(&command_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn Roslyn server process")?;

    let mut server_stdin = server_process
        .stdin
        .take()
        .context("Failed to open server stdin")?;
    let server_stdout = server_process
        .stdout
        .take()
        .context("Failed to open server stdout")?;
    let server_stderr = server_process
        .stderr
        .take()
        .context("Failed to open server stderr")?;

    let stderr_logger = logger.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(server_stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            stderr_logger.log(&format!("[Roslyn] {}", line));
        }
    });

    let mut client_stdin = BufReader::new(io::stdin());
    let mut client_stdout = io::stdout();

    let stream_to_stdout = async {
        let mut reader = BufReader::new(server_stdout);
        io::copy(&mut reader, &mut client_stdout).await
    };

    let stdin_logger = logger.clone();
    let stdin_to_stream = async {
        loop {
            let mut buffer = vec![0; 8192];
            let bytes_read = client_stdin
                .read(&mut buffer)
                .await
                .expect("Failed to read from client");

            if bytes_read == 0 {
                break;
            }

            server_stdin
                .write_all(&buffer[..bytes_read])
                .await
                .expect("Failed to write to server");

            let content = String::from_utf8_lossy(&buffer[..bytes_read]);

            if content.contains("\"method\":\"initialize\"") {
                stdin_logger.log("Detected initialize request");
                if let Some(solution_notification) =
                    extract_and_create_solution_notification(&content, &stdin_logger)
                {
                    stdin_logger.log("Will inject solution notification after 'initialized'");

                    loop {
                        let mut buffer = vec![0; 8192];
                        let bytes_read = client_stdin
                            .read(&mut buffer)
                            .await
                            .expect("Failed to read from client");

                        if bytes_read == 0 {
                            break;
                        }

                        server_stdin
                            .write_all(&buffer[..bytes_read])
                            .await
                            .expect("Failed to write to server");

                        let content = String::from_utf8_lossy(&buffer[..bytes_read]);

                        if content.contains("\"method\":\"initialized\"") {
                            stdin_logger
                                .log("Detected initialized notification, injecting solution/open");
                            server_stdin
                                .write_all(solution_notification.as_bytes())
                                .await
                                .expect("Failed to inject solution notification");
                            break;
                        }
                    }
                }
                break;
            }
        }

        io::copy(&mut client_stdin, &mut server_stdin).await
    };

    tokio::select! {
        result = stdin_to_stream => {
            if let Err(e) = result {
                logger.log(&format!("stdin_to_stream error: {}", e));
            }
        }
        result = stream_to_stdout => {
            if let Err(e) = result {
                logger.log(&format!("stream_to_stdout error: {}", e));
            }
        }
    }

    logger.log("Proxy shutting down");
    server_process.kill().await.ok();

    Ok(())
}

fn extract_and_create_solution_notification(init_message: &str, logger: &Logger) -> Option<String> {
    let json_start = init_message.find('{')?;
    let parsed: Value = serde_json::from_str(&init_message[json_start..]).ok()?;

    let root_path = parsed["params"]["rootUri"]
        .as_str()
        .and_then(|uri| url::Url::parse(uri).ok())
        .and_then(|url| url.to_file_path().ok())
        .or_else(|| parsed["params"]["rootPath"].as_str().map(PathBuf::from))?;

    logger.log(&format!("Workspace root: {}", root_path.display()));

    if let Some(solution_path) = find_solution(&root_path) {
        logger.log(&format!("Found solution: {}", solution_path.display()));
        let solution_uri = url::Url::from_file_path(&solution_path).ok()?;

        let notification = json!({
            "jsonrpc": "2.0",
            "method": "solution/open",
            "params": {
                "solution": solution_uri.to_string()
            }
        });

        let notification_str = serde_json::to_string(&notification).ok()?;
        let content_length = notification_str.len();
        return Some(format!(
            "Content-Length: {}\r\n\r\n{}",
            content_length, notification_str
        ));
    }

    if let Some(projects) = find_projects(&root_path) {
        logger.log(&format!("Found {} projects", projects.len()));

        let notification = json!({
            "jsonrpc": "2.0",
            "method": "project/open",
            "params": {
                "projects": projects
            }
        });

        let notification_str = serde_json::to_string(&notification).ok()?;
        let content_length = notification_str.len();
        return Some(format!(
            "Content-Length: {}\r\n\r\n{}",
            content_length, notification_str
        ));
    }

    None
}

fn find_solution(root: &PathBuf) -> Option<PathBuf> {
    let extensions = ["sln", "slnx"];

    for entry in walkdir::WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if let Some(ext) = entry.path().extension() {
            if extensions.iter().any(|&e| e == ext) {
                return Some(entry.path().to_path_buf());
            }
        }
    }

    None
}

fn find_projects(root: &PathBuf) -> Option<Vec<String>> {
    let mut projects = Vec::new();

    for entry in walkdir::WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if let Some(ext) = entry.path().extension() {
            if ext == "csproj" {
                if let Ok(uri) = url::Url::from_file_path(entry.path()) {
                    projects.push(uri.to_string());
                }
            }
        }
    }

    if projects.is_empty() {
        None
    } else {
        Some(projects)
    }
}

fn find_dotnet() -> Result<String> {
    #[cfg(windows)]
    let which_command = "where";
    #[cfg(not(windows))]
    let which_command = "which";

    if let Ok(output) = std::process::Command::new(which_command)
        .arg("dotnet")
        .output()
    {
        if output.status.success() {
            if let Ok(path) = String::from_utf8(output.stdout) {
                let path = path.lines().next().unwrap_or("").trim();
                if !path.is_empty() {
                    return Ok(path.to_string());
                }
            }
        }
    }

    #[cfg(windows)]
    let common_paths = vec![
        "C:\\Program Files\\dotnet\\dotnet.exe",
        "C:\\Program Files (x86)\\dotnet\\dotnet.exe",
    ];

    #[cfg(not(windows))]
    let common_paths = vec![
        "/usr/local/share/dotnet/dotnet",
        "/usr/local/bin/dotnet",
        "/usr/bin/dotnet",
        "/opt/homebrew/bin/dotnet",
    ];

    for path in common_paths {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    anyhow::bail!("dotnet executable not found in PATH or common locations")
}

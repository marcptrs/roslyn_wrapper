// Logging module controlled by LSP initialization options.
// Defaults: level=info, file=./roslyn_wrapper.log unless reconfigured at runtime.
use chrono::{Local, SecondsFormat};
use once_cell::sync::Lazy;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOG_SINK: Lazy<Mutex<LogSink>> = Lazy::new(|| Mutex::new(LogSink::new(default_log_file_path())));

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
enum LogLevel {
    Off = 0,
    Error = 1,
    Info = 2,
    Debug = 3,
}

#[derive(Clone, Debug)]
struct LogConfig {
    level: LogLevel,
    file_path: PathBuf,
}

static LOG_CONFIG: Lazy<Mutex<LogConfig>> = Lazy::new(|| Mutex::new(LogConfig {
    level: LogLevel::Info,
    file_path: default_log_file_path(),
}));

fn parse_level(s: &str) -> LogLevel {
    match s.to_lowercase().as_str() {
        "off" | "none" => LogLevel::Off,
        "error" => LogLevel::Error,
        "info" => LogLevel::Info,
        "debug" => LogLevel::Debug,
        _ => LogLevel::Info,
    }
}

fn should_log(level: LogLevel) -> bool {
    let cfg = LOG_CONFIG.lock().unwrap();
    cfg.level >= level
}

pub fn configure(level: Option<&str>, file_path: Option<&str>, directory: Option<&str>) {
    let mut cfg = LOG_CONFIG.lock().unwrap();

    if let Some(level_str) = level {
        cfg.level = parse_level(level_str);
    }

    if let Some(path_str) = file_path {
        if !path_str.trim().is_empty() {
            cfg.file_path = PathBuf::from(path_str);
        }
    } else if let Some(dir_str) = directory {
        if !dir_str.trim().is_empty() {
            cfg.file_path = PathBuf::from(dir_str).join("roslyn_wrapper.log");
        }
    }

    if let Ok(mut sink) = LOG_SINK.lock() {
        sink.reopen(cfg.file_path.clone());
        // Emit a line to confirm reconfiguration
        let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        if let Some(f) = sink.file.as_mut() {
            let _ = writeln!(
                f,
                "[{}] [roslyn_wrapper] Logger reconfigured (level: {:?}, path: {})",
                timestamp,
                cfg.level,
                cfg.file_path.display()
            );
            let _ = f.flush();
        }
    }
}

pub fn log_line(message: impl AsRef<str>) {
    if should_log(LogLevel::Info) {
        if let Ok(mut sink) = LOG_SINK.lock() {
            sink.write_str(message.as_ref());
        }
    }
}

pub fn info(message: impl AsRef<str>) {
    log_line(message);
}

pub fn debug(message: impl AsRef<str>) {
    if should_log(LogLevel::Debug) {
        if let Ok(mut sink) = LOG_SINK.lock() {
            sink.write_str(message.as_ref());
        }
    }
}

pub fn error(message: impl AsRef<str>) {
    if should_log(LogLevel::Error) {
        if let Ok(mut sink) = LOG_SINK.lock() {
            sink.write_str(message.as_ref());
        }
    }
}

struct LogSink {
    file: Option<File>,
}

impl LogSink {
    fn new(path: PathBuf) -> Self {
        let mut file = initialize_file(&path);
        if let Some(file_handle) = file.as_mut() {
            let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let _ = writeln!(
                file_handle,
                "[{}] [roslyn_wrapper] Logger initialized (path: {})",
                timestamp,
                path.display()
            );
            let _ = file_handle.flush();
        }
        Self { file }
    }

    fn reopen(&mut self, path: PathBuf) {
        self.file = initialize_file(&path);
    }

    fn write_str(&mut self, message: &str) {
        if let Some(file) = self.file.as_mut() {
            let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            for line in message.lines() {
                let _ = writeln!(file, "[{timestamp}] {line}");
            }
            let _ = file.flush();
        }
    }
}

fn default_log_file_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("roslyn_wrapper.log")
}

fn initialize_file(path: &PathBuf) -> Option<File> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            if let Err(_err) = std::fs::create_dir_all(parent) {
                return None;
            }
        }
    }

    OpenOptions::new().create(true).append(true).open(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn configure_updates_path_and_level() {
        let tmp = tempdir().unwrap();
        let log_path = tmp.path().join("x.log");
        configure(Some("debug"), Some(log_path.to_str().unwrap()), None);
        // Write a debug message; ensure no panic
        debug("[roslyn_wrapper] test debug");
    }

    #[test]
    fn configure_directory_sets_default_filename() {
        let tmp = tempdir().unwrap();
        configure(Some("info"), None, Some(tmp.path().to_str().unwrap()));
        info("[roslyn_wrapper] test info");
    }
}

// Logging module
// Environment variables:
// - ROSLYN_WRAPPER_LOG_LEVEL: off|error|info|debug (default: info)
// - ROSLYN_WRAPPER_LOG_PATH: explicit log file path; overrides CWD
// - ROSLYN_WRAPPER_CWD: directory to place roslyn_wrapper.log if LOG_PATH not set
// All log lines prefixed with timestamp; info/debug/error filtering is runtime-controlled.
use chrono::{Local, SecondsFormat};
use once_cell::sync::Lazy;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOG_SINK: Lazy<Mutex<LogSink>> = Lazy::new(|| Mutex::new(LogSink::new()));

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
enum LogLevel {
    Off = 0,
    Error = 1,
    Info = 2,
    Debug = 3,
}

static LOG_LEVEL: Lazy<LogLevel> = Lazy::new(|| {
    match std::env::var("ROSLYN_WRAPPER_LOG_LEVEL")
        .unwrap_or_else(|_| "info".to_string())
        .to_lowercase()
        .as_str()
    {
        "off" | "none" => LogLevel::Off,
        "error" => LogLevel::Error,
        "info" => LogLevel::Info,
        "debug" => LogLevel::Debug,
        _ => LogLevel::Info,
    }
});

fn should_log(level: LogLevel) -> bool {
    *LOG_LEVEL >= level
}

pub fn log_line(message: impl AsRef<str>) {
    // Backwards compatibility: treat as info-level
    if should_log(LogLevel::Info) {
        if let Ok(mut sink) = LOG_SINK.lock() {
            sink.write_str(message.as_ref());
        }
    }
}

pub fn info(message: impl AsRef<str>) { log_line(message); }

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
    fn new() -> Self {
        let path = resolve_log_path();
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

    fn write_str(&mut self, message: &str) {
        if let Some(file) = self.file.as_mut() {
            let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            for line in message.lines() {
                let _ = writeln!(file, "[{}] {}", timestamp, line);
            }
            let _ = file.flush();
        }
    }
}

fn resolve_log_path() -> PathBuf {
    if let Ok(path) = std::env::var("ROSLYN_WRAPPER_LOG_PATH") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }

    if let Ok(cwd) = std::env::var("ROSLYN_WRAPPER_CWD") {
        if !cwd.trim().is_empty() {
            return PathBuf::from(cwd).join("roslyn_wrapper.log");
        }
    }

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

    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => Some(file),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::env;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    static ENV_GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[test]
    fn resolve_log_path_prefers_explicit_env() {
        let _guard = ENV_GUARD.lock().unwrap();
        let temp_dir = tempdir().unwrap();
        let explicit_path = temp_dir.path().join("custom.log");

        env::set_var("ROSLYN_WRAPPER_LOG_PATH", &explicit_path);
        env::remove_var("ROSLYN_WRAPPER_CWD");

        let resolved = resolve_log_path();
        assert_eq!(resolved, explicit_path);

        env::remove_var("ROSLYN_WRAPPER_LOG_PATH");
    }

    #[test]
    fn resolve_log_path_falls_back_to_cwd() {
        let _guard = ENV_GUARD.lock().unwrap();
        let temp_dir = tempdir().unwrap();
        env::remove_var("ROSLYN_WRAPPER_LOG_PATH");
        env::set_var("ROSLYN_WRAPPER_CWD", temp_dir.path());

        let resolved = resolve_log_path();
        assert_eq!(resolved, temp_dir.path().join("roslyn_wrapper.log"));

        env::remove_var("ROSLYN_WRAPPER_CWD");
    }

    #[cfg(unix)]
    #[test]
    fn log_sink_gracefully_handles_unwritable_path() {
        let _guard = ENV_GUARD.lock().unwrap();
        let temp_dir = tempdir().unwrap();
        let readonly_dir = temp_dir.path().join("readonly");
        std::fs::create_dir(&readonly_dir).unwrap();

        let mut perms = std::fs::metadata(&readonly_dir).unwrap().permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o500);
        std::fs::set_permissions(&readonly_dir, perms.clone()).unwrap();

        let log_path = readonly_dir.join("test.log");
        env::set_var("ROSLYN_WRAPPER_LOG_PATH", &log_path);
        env::remove_var("ROSLYN_WRAPPER_CWD");

        let sink = LogSink::new();
        assert!(sink.file.is_none());
        drop(sink);

        let mut restore = std::fs::metadata(&readonly_dir).unwrap().permissions();
        restore.set_mode(original_mode);
        std::fs::set_permissions(&readonly_dir, restore).unwrap();

        env::remove_var("ROSLYN_WRAPPER_LOG_PATH");
    }
}

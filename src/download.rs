use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

// Use stable version from nuget.org (public, no authentication required)
const ROSLYN_VERSION: &str = "5.0.0-1.25277.114";

// LSP Message Type Constants
const LSP_MESSAGE_TYPE_INFO: i64 = 3;

/// Send an LSP window/showMessage notification to stderr
/// This allows the wrapper to communicate status to Zed before LSP initialization
fn send_lsp_notification(message: &str) {
    let notification = format!(
        r#"{{"jsonrpc":"2.0","method":"window/showMessage","params":{{"type":{},"message":"{}"}}}}"#,
        LSP_MESSAGE_TYPE_INFO,
        message.replace('"', "\\\"")
    );
    
    // Send to stderr so it doesn't interfere with LSP protocol on stdout
    let _ = writeln!(std::io::stderr(), "{}", notification);
    let _ = std::io::stderr().flush();
}

/// Get the cache directory for storing Roslyn
pub fn get_cache_dir() -> Result<PathBuf> {
    let cache_dir = ProjectDirs::from("com", "github", "roslyn-wrapper")
        .ok_or_else(|| anyhow!("Unable to find cache directory"))?
        .cache_dir()
        .to_path_buf();

    // Validate Windows path length limit (260 characters)
    #[cfg(windows)]
    {
        if let Some(path_str) = cache_dir.to_str() {
            // Windows MAX_PATH is 260 characters, but we need buffer for version subdirs
            // Typical structure: C:\Users\username\AppData\Local\roslyn-wrapper\cache\{version}\...
            if path_str.len() > 200 {
                return Err(anyhow!(
                    "Cache directory path exceeds safe Windows length limit ({}): {}",
                    path_str.len(),
                    path_str
                ));
            }
        }
    }

    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

/// Clean up old cached versions, keeping only the latest
fn cleanup_old_versions(cache_dir: &Path, latest_version: &str) -> Result<()> {
    if !cache_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(cache_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                // Remove directories that aren't the latest version or temp files
                if !dir_name.starts_with(".tmp_") && dir_name != latest_version {
                    match fs::remove_dir_all(&path) {
                        Ok(_) => {
                            crate::logger::info(format!(
                                "[roslyn_wrapper] Cleaned up old version: {dir_name}"
                            ));
                        }
                        Err(e) => {
                            crate::logger::debug(format!(
                                "[roslyn_wrapper] Failed to clean old version {dir_name}: {e}"
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Get the path to the Roslyn binary
pub async fn get_roslyn_path() -> Result<PathBuf> {
    let cache_dir = get_cache_dir()?;

    // Check if version is already cached
    let version_dir = cache_dir.join(ROSLYN_VERSION);
    if let Ok(binary_path) = find_binary_in_dir(&version_dir) {
        crate::logger::info(format!(
            "[roslyn_wrapper] Using cached Roslyn {ROSLYN_VERSION}"
        ));
        send_lsp_notification("Roslyn LSP is ready");
        return Ok(binary_path);
    }

    // Try to download the version
    send_lsp_notification(&format!("Downloading Roslyn LSP {}...", ROSLYN_VERSION));
    crate::logger::info(format!(
        "[roslyn_wrapper] Downloading Roslyn {ROSLYN_VERSION} from nuget.org"
    ));

    if let Ok(()) = download_and_extract_roslyn(&version_dir, ROSLYN_VERSION).await {
        crate::logger::debug("[roslyn_wrapper] Download and extraction succeeded");

        // Clean up old versions now that we have the current one
        let _ = cleanup_old_versions(&cache_dir, ROSLYN_VERSION);

        // Search for the binary after extraction
        if let Ok(binary_path) = find_binary_in_dir(&version_dir) {
            crate::logger::info(format!(
                "[roslyn_wrapper] Installed Roslyn {ROSLYN_VERSION}"
            ));
            send_lsp_notification("Roslyn LSP installation complete");
            return Ok(binary_path);
        } else {
            crate::logger::error("[roslyn_wrapper] Binary not found after extraction");
            send_lsp_notification("Error: Roslyn binary not found after extraction");
        }
    } else {
        crate::logger::error("[roslyn_wrapper] Failed to download Roslyn");
        send_lsp_notification("Download failed, checking for global installation...");
    }

    // Fallback: Try to use globally installed Roslyn via dotnet tool
    send_lsp_notification("Checking for globally installed Roslyn...");
    crate::logger::info("[roslyn_wrapper] Checking for globally installed Roslyn");
    if let Ok(global_path) = find_global_roslyn() {
        crate::logger::info("[roslyn_wrapper] Using globally installed Roslyn");
        send_lsp_notification("Using globally installed Roslyn LSP");
        return Ok(global_path);
    }

    send_lsp_notification("Error: Failed to download or find Roslyn LSP");
    Err(anyhow!(
        "Failed to find or download Roslyn LSP. Please ensure:\n\
         1. You have internet access for NuGet downloads, or\n\
         2. Install manually: dotnet tool install --global Microsoft.CodeAnalysis.LanguageServer"
    ))
}

/// Get the binary path for a given version directory
/// Search recursively for the Roslyn language server binary in a directory
fn find_binary_in_dir(dir: &Path) -> Result<PathBuf> {
    let binary_name = if cfg!(windows) {
        "Microsoft.CodeAnalysis.LanguageServer.exe"
    } else {
        "Microsoft.CodeAnalysis.LanguageServer"
    };

    // Walk the directory tree looking for the binary
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if entry.file_name() == binary_name {
            let path = entry.path().to_path_buf();
            crate::logger::debug("[roslyn_wrapper] Found binary");
            return Ok(path);
        }
    }

    Err(anyhow!(
        "Binary {} not found in {}",
        binary_name,
        dir.display()
    ))
}

/// Try to find globally installed Roslyn from dotnet tool
fn find_global_roslyn() -> Result<PathBuf> {
    // Common paths where dotnet tools are installed
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;

    #[cfg(windows)]
    let mut possible_paths = vec![
        home_dir.join(".dotnet/tools/Microsoft.CodeAnalysis.LanguageServer"),
    ];
    #[cfg(not(windows))]
    let possible_paths = vec![
        home_dir.join(".dotnet/tools/Microsoft.CodeAnalysis.LanguageServer"),
    ];

    // On Windows, also try using USERPROFILE environment variable as fallback
    #[cfg(windows)]
    {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            possible_paths.push(
                PathBuf::from(userprofile)
                    .join(".dotnet/tools/Microsoft.CodeAnalysis.LanguageServer.exe"),
            );
        }
    }

    for path in possible_paths {
        if path.exists() {
            return Ok(path);
        }
    }

    Err(anyhow!("Global Roslyn installation not found"))
}

/// Download Roslyn from Azure DevOps NuGet feed and extract it
async fn download_and_extract_roslyn(target_dir: &Path, version: &str) -> Result<()> {
    fs::create_dir_all(target_dir)?;

    let rid = get_platform_rid();
    let package_name = format!("Microsoft.CodeAnalysis.LanguageServer.{rid}");

    // Use Azure DevOps NuGet v3 flat container URL (lowercase package name)
    let package_name_lower = package_name.to_lowercase();
    let nuget_url = format!(
        "https://pkgs.dev.azure.com/azure-public/vside/_packaging/msft_consumption/nuget/v3/flat2/{package_name_lower}/{version}/{package_name_lower}.{version}.nupkg"
    );

    crate::logger::debug(format!("[roslyn_wrapper] Download URL: {nuget_url}"));

    let client = reqwest::Client::new();
    let response = client.get(&nuget_url).send().await.map_err(|e| {
        let error_msg = format!("Network error downloading Roslyn: {}", e);
        send_lsp_notification(&error_msg);
        anyhow!(error_msg)
    })?;

    if !response.status().is_success() {
        let error_msg = format!(
            "Failed to download Roslyn {}: HTTP {}",
            version,
            response.status()
        );
        send_lsp_notification(&error_msg);
        return Err(anyhow!(error_msg));
    }

    let bytes = response.bytes().await?;
    crate::logger::debug(format!(
        "[roslyn_wrapper] Download size {} bytes",
        bytes.len()
    ));

    send_lsp_notification("Extracting Roslyn LSP...");

    // Extract to temporary location first
    let temp_path = target_dir
        .parent()
        .ok_or_else(|| anyhow!("Failed to get parent directory of target path"))?
        .join(format!(".tmp_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_path)?;

    // NuGet packages are always ZIP files
    extract_zip(&bytes, &temp_path)?;

    // Move from temp to final location
    crate::logger::debug("[roslyn_wrapper] Moving extracted files");
    let mut copied_count = 0;
    for entry in walkdir::WalkDir::new(&temp_path) {
        let entry = entry?;
        let rel_path = entry
            .path()
            .strip_prefix(&temp_path)
            .unwrap_or(entry.path());
        let target_path = target_dir.join(rel_path);

        if entry.path().is_dir() {
            fs::create_dir_all(&target_path)?;
        } else {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target_path)?;
            copied_count += 1;
        }
    }
    crate::logger::debug(format!("[roslyn_wrapper] Copied {copied_count} files"));

    fs::remove_dir_all(temp_path)?;
    crate::logger::debug("[roslyn_wrapper] Extraction complete");

    Ok(())
}

/// Extract a ZIP archive and copy LanguageServer files to temp directory
fn extract_zip(bytes: &[u8], temp_path: &Path) -> Result<()> {
    let mut zip = ZipArchive::new(std::io::Cursor::new(bytes))?;

    // Find and extract LanguageServer files
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let file_path = file.name().to_string();

        // Look for files in the content/LanguageServer directory
        if file_path.contains("content/LanguageServer") {
            let relative_path = file_path
                .split("content/LanguageServer/")
                .last()
                .unwrap_or("");

            if !relative_path.is_empty() && !file.is_dir() {
                let target_file_path = temp_path.join(relative_path);
                if let Some(parent) = target_file_path.parent() {
                    fs::create_dir_all(parent)?;
                }

                let mut target_file = fs::File::create(&target_file_path)?;
                std::io::copy(&mut file, &mut target_file)?;

                // Make executable on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if relative_path.ends_with("Microsoft.CodeAnalysis.LanguageServer") {
                        let perms = fs::Permissions::from_mode(0o755);
                        fs::set_permissions(&target_file_path, perms)?;
                    }
                }

                crate::logger::debug(format!("[roslyn_wrapper] Extracted: {relative_path}"));
            }
        }
    }

    Ok(())
}

/// Get platform-specific runtime identifier (RID)
/// NuGet packages (.nupkg) are always ZIP files, so we only need the RID.
fn get_platform_rid() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "win-x64";
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "win-arm64";
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "linux-x64";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "linux-arm64";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "osx-x64";
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "osx-arm64";
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    "neutral"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_info() {
        let rid = get_platform_rid();
        assert!(!rid.is_empty());
        println!("Platform RID: {rid}");
    }
}

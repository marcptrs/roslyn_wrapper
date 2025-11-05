use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};
use zip::ZipArchive;
use flate2::read::GzDecoder;
use tar::Archive as TarArchive;

// Try these versions in order until one works
const ROSLYN_VERSIONS: &[&str] = &[
    "5.0.0-1.25277.114",
    "4.12.0",
    "4.11.0",
    "4.10.0",
];

/// Get the cache directory for storing Roslyn
pub fn get_cache_dir() -> Result<PathBuf> {
    let cache_dir = ProjectDirs::from("com", "github", "roslyn-wrapper")
        .ok_or_else(|| anyhow!("Unable to find cache directory"))?
        .cache_dir()
        .to_path_buf();

    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

/// Get the path to the Roslyn binary
pub async fn get_roslyn_path() -> Result<PathBuf> {
    let cache_dir = get_cache_dir()?;

    // Try each cached version first
    for version in ROSLYN_VERSIONS {
        let version_dir = cache_dir.join(version);
        
        // Search for the binary in this version directory
        if let Ok(binary_path) = find_binary_in_dir(&version_dir) {
            eprintln!(
                "[roslyn-wrapper] Found cached Roslyn {} at: {}",
                version,
                binary_path.display()
            );
            return Ok(binary_path);
        }
    }

    // Try to download versions in order
    for version in ROSLYN_VERSIONS {
        let version_dir = cache_dir.join(version);

        eprintln!(
            "[roslyn-wrapper] Trying to download Roslyn {} from NuGet...",
            version
        );

        if let Ok(()) = download_and_extract_roslyn(&version_dir, version).await {
            eprintln!("[roslyn-wrapper] download_and_extract_roslyn succeeded, searching for binary");
            // Search for the binary after extraction
            if let Ok(binary_path) = find_binary_in_dir(&version_dir) {
                eprintln!(
                    "[roslyn-wrapper] Successfully installed Roslyn {} at: {}",
                    version,
                    binary_path.display()
                );
                return Ok(binary_path);
            } else {
                eprintln!("[roslyn-wrapper] Binary not found in directory after extraction!");
            }
        } else {
            eprintln!("[roslyn-wrapper] download_and_extract_roslyn failed");
        }
        eprintln!(
            "[roslyn-wrapper] Failed to download version {}, trying next...",
            version
        );
    }

    // Fallback: Try to use globally installed Roslyn via dotnet tool
    eprintln!("[roslyn-wrapper] Trying to find globally installed Roslyn...");
    if let Ok(global_path) = find_global_roslyn() {
        eprintln!(
            "[roslyn-wrapper] Found globally installed Roslyn at: {}",
            global_path.display()
        );
        return Ok(global_path);
    }

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
    for entry in walkdir::WalkDir::new(dir) {
        if let Ok(entry) = entry {
            if entry.file_name() == binary_name {
                let path = entry.path().to_path_buf();
                eprintln!("[roslyn-wrapper] Found binary at: {}", path.display());
                return Ok(path);
            }
        }
    }

    Err(anyhow!("Binary {} not found in {}", binary_name, dir.display()))
}

/// Try to find globally installed Roslyn from dotnet tool
fn find_global_roslyn() -> Result<PathBuf> {
    // Common paths where dotnet tools are installed
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;

    let possible_paths = if cfg!(windows) {
        vec![
            home_dir.join(".dotnet/tools/Microsoft.CodeAnalysis.LanguageServer.exe"),
            PathBuf::from("C:/Users")
                .join(std::env::var("USERNAME").unwrap_or_default())
                .join(".dotnet/tools/Microsoft.CodeAnalysis.LanguageServer.exe"),
        ]
    } else {
        vec![
            home_dir.join(".dotnet/tools/Microsoft.CodeAnalysis.LanguageServer"),
        ]
    };

    for path in possible_paths {
        if path.exists() {
            return Ok(path);
        }
    }

    Err(anyhow!("Global Roslyn installation not found"))
}

/// Download Roslyn from NuGet and extract it
async fn download_and_extract_roslyn(target_dir: &Path, version: &str) -> Result<()> {
    fs::create_dir_all(target_dir)?;

    let (rid, extension) = get_platform_info();
    let package_name = format!("Microsoft.CodeAnalysis.LanguageServer.{}", rid);
    let nuget_url = format!(
        "https://www.nuget.org/api/v2/package/{}/{}",
        package_name, version
    );

    eprintln!("[roslyn-wrapper] Downloading from: {}", nuget_url);

    let client = reqwest::Client::new();
    let response = client.get(&nuget_url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to download Roslyn {}: HTTP {}",
            version,
            response.status()
        ));
    }

    let bytes = response.bytes().await?;
    eprintln!("[roslyn-wrapper] Downloaded {} bytes", bytes.len());

    // Extract to temporary location first
    let temp_path = target_dir
        .parent()
        .unwrap()
        .join(format!(".tmp_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_path)?;

    match extension {
        "zip" => {
            extract_zip(&bytes, &temp_path)?;
        }
        "tar.gz" => {
            extract_tar_gz(&bytes, &temp_path)?;
        }
        _ => {
            return Err(anyhow!("Unsupported archive format: {}", extension));
        }
    }

    // Move from temp to final location
    eprintln!("[roslyn-wrapper] Moving extracted files from temp to: {}", target_dir.display());
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
            fs::create_dir_all(target_path.parent().unwrap())?;
            fs::copy(entry.path(), &target_path)?;
            copied_count += 1;
        }
    }
    eprintln!("[roslyn-wrapper] Copied {} files to {}", copied_count, target_dir.display());

    fs::remove_dir_all(temp_path)?;
    eprintln!("[roslyn-wrapper] Extraction complete");

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
                fs::create_dir_all(target_file_path.parent().unwrap())?;

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

                eprintln!("[roslyn-wrapper] Extracted: {}", relative_path);
            }
        }
    }

    Ok(())
}

/// Extract a tar.gz archive and copy LanguageServer files to temp directory
fn extract_tar_gz(bytes: &[u8], temp_path: &Path) -> Result<()> {
    let gz_decoder = GzDecoder::new(std::io::Cursor::new(bytes));
    let mut tar_archive = TarArchive::new(gz_decoder);

    // Extract all entries
    for entry_result in tar_archive.entries()? {
        let mut entry = entry_result?;
        let path = entry.path()?.to_path_buf();
        let path_str = path.to_str().unwrap_or("");

        // Look for files in the content/LanguageServer directory
        if path_str.contains("content/LanguageServer") {
            let relative_path = path_str
                .split("content/LanguageServer/")
                .last()
                .unwrap_or("");

            if !relative_path.is_empty() {
                let target_file_path = temp_path.join(relative_path);
                
                if entry.header().entry_type().is_dir() {
                    fs::create_dir_all(&target_file_path)?;
                } else {
                    fs::create_dir_all(target_file_path.parent().unwrap())?;
                    entry.unpack(&target_file_path)?;

                    // Make executable on Unix
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if relative_path.ends_with("Microsoft.CodeAnalysis.LanguageServer") {
                            let perms = fs::Permissions::from_mode(0o755);
                            fs::set_permissions(&target_file_path, perms)?;
                        }
                    }

                    eprintln!("[roslyn-wrapper] Extracted: {}", relative_path);
                }
            }
        }
    }

    Ok(())
}

/// Get platform-specific runtime identifier and archive extension
fn get_platform_info() -> (&'static str, &'static str) {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return ("win-x64", "zip");

    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return ("win-arm64", "zip");

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return ("linux-x64", "tar.gz");

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return ("linux-arm64", "tar.gz");

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return ("osx-x64", "tar.gz");

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return ("osx-arm64", "tar.gz");

    // Default fallback for unsupported platforms
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    ("neutral", "tar.gz")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_info() {
        let (rid, _ext) = get_platform_info();
        assert!(!rid.is_empty());
        println!("Platform RID: {}", rid);
    }
}

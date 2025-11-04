# Build script for roslyn-wrapper on Windows
# This script builds the binary and copies it to the locations needed for local testing

param(
    [switch]$Release = $true,
    [switch]$Clean = $false
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = $ScriptDir
$TargetDir = Join-Path $ProjectRoot "target"

if ($Clean) {
    Write-Host "[*] Cleaning build artifacts..."
    cargo clean
    Write-Host "[OK] Cleaned"
    Write-Host ""
}

Write-Host "[*] Building roslyn-wrapper..."
if ($Release) {
    cargo build --release
}
else {
    cargo build
}

$BuildType = if ($Release) { "release" } else { "debug" }
$BinaryName = "roslyn-wrapper.exe"
$BinaryPath = Join-Path -Path (Join-Path -Path $TargetDir -ChildPath $BuildType) -ChildPath $BinaryName

if (-not (Test-Path $BinaryPath)) {
    Write-Host "[ERROR] Binary not found at $BinaryPath"
    exit 1
}

Write-Host "[OK] Binary built successfully: $BinaryPath"
Write-Host ""

# Copy to cache locations for local testing
Write-Host "[*] Copying binary to local cache locations for testing..."

# Use LocalAppData for Windows cache
$CacheDir = Join-Path $env:LOCALAPPDATA "roslyn-wrapper\bin\0.1.0"
$CacheDirAlt = Join-Path $env:TEMP "roslyn-wrapper\bin\0.1.0"

# Create cache directories
New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null
New-Item -ItemType Directory -Force -Path $CacheDirAlt | Out-Null

# Copy binary
Copy-Item -Path $BinaryPath -Destination (Join-Path $CacheDir $BinaryName) -Force
Copy-Item -Path $BinaryPath -Destination (Join-Path $CacheDirAlt $BinaryName) -Force

Write-Host "[OK] Copied to $CacheDir"
Write-Host "[OK] Copied to $CacheDirAlt"

# Also copy to Zed's extension work directory if it exists
$ZedExtDir = Join-Path $env:LOCALAPPDATA "Zed\extensions\work\csharp_roslyn"
if (Test-Path $ZedExtDir) {
    # Extension expects binary at: roslyn-wrapper-0.1.0/roslyn-wrapper.exe
    $ExtCacheDir = Join-Path $ZedExtDir "roslyn-wrapper-0.1.0"
    New-Item -ItemType Directory -Force -Path $ExtCacheDir | Out-Null
    Copy-Item -Path $BinaryPath -Destination (Join-Path $ExtCacheDir $BinaryName) -Force
    Write-Host "[OK] Copied to Zed extension directory: $ExtCacheDir"
    Write-Host "    Binary at: $(Join-Path $ExtCacheDir $BinaryName)"
    
    # Also copy to root of work directory as fallback (when download fails)
    Copy-Item -Path $BinaryPath -Destination (Join-Path $ZedExtDir $BinaryName) -Force
    Write-Host "[OK] Also copied to: $(Join-Path $ZedExtDir $BinaryName) (fallback location)"
}
else {
    Write-Host "[INFO] Zed extension directory not found at $ZedExtDir"
    Write-Host "       Extension will be created when you install it in Zed"
}

Write-Host ""
Write-Host "[OK] Build complete!"
Write-Host "Location: $BinaryPath"
Write-Host "Cached:   $CacheDir"
Write-Host "Cached:   $CacheDirAlt"
if (Test-Path $ZedExtDir) {
    Write-Host "Cached:   $ExtCacheDir"
}

Write-Host ""
Write-Host "[TEST] To test locally:"
Write-Host "  1. Ensure Zed is closed"
Write-Host "  2. Run: zed C:\projects\zed\TestCSharpProject"
Write-Host "  3. Open HelloWorld/Program.cs"
Write-Host "  4. Check Help -> View Logs for [roslyn-wrapper] messages"

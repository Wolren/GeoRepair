param(
  [string]$Version = "0.2.4"
)

$ErrorActionPreference = "Stop"

$Target = "patches\geos-src"
$TargetOk = Join-Path $Target ".cargo-ok"

# Idempotency check — already present
if ((Test-Path $Target) -and (Test-Path $TargetOk)) {
  Write-Host "✓ geos-src v$Version already present at $Target"
  exit 0
}

Write-Host "Downloading geos-src v$Version from crates.io ..."
New-Item -ItemType Directory -Force -Path "patches" | Out-Null

$TempDir = Join-Path $env:TEMP "geos-src-$([System.Guid]::NewGuid())"
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
$CrateFile = Join-Path $TempDir "geos-src.crate"

# Download from crates.io CDN
$urls = @(
  "https://static.crates.io/crates/geos-src/geos-src-$Version.crate",
  "https://crates.io/api/v1/crates/geos-src/$Version/download"
)

$downloaded = $false
foreach ($url in $urls) {
  try {
    Write-Host "  Trying: $url"
    Invoke-WebRequest -Uri $url -OutFile $CrateFile -UseBasicParsing -ErrorAction Stop
    if ((Get-Item $CrateFile).Length -gt 0) {
      $downloaded = $true
      break
    }
  } catch {
    # Try next URL
  }
}

if (-not $downloaded) {
  Write-Error "✗ Failed to download geos-src v$Version from crates.io"
  Write-Error "  Check your network connection and that the version exists:"
  Write-Error "  https://crates.io/crates/geos-src/$Version"
  Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
  exit 1
}

# Extract — crates.io archives produce geos-src-$Version/
Write-Host "  Extracting ..."
tar xzf $CrateFile -C patches/

# Rename to canonical path
$ExtractedDir = Join-Path "patches" "geos-src-$Version"
if (Test-Path $ExtractedDir) {
  # Remove partial extraction from a previous failed run, if any
  if (Test-Path $Target) {
    Remove-Item -Recurse -Force $Target
  }
  Move-Item $ExtractedDir $Target
}

# Create marker so idempotency check works on re-runs
New-Item -ItemType File -Force -Path $TargetOk | Out-Null

# Cleanup
Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "✓ geos-src v$Version installed at $Target"
Write-Host ""
Write-Host "Now run GEOS comparison benchmarks:"
Write-Host "  cargo bench --features bench-geos"
Write-Host ""
Write-Host "Or use the existing helper:"
Write-Host "  .\scripts\bench-geos.ps1"

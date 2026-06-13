param(
  [string]$Bench = "real_world",
  [string]$Dataset = "",
  [string]$Features = "bench-geos,arrange,structure,parallel,simd"
)

$ErrorActionPreference = "Stop"

# Try to detect conda GEOS
$condaBase = $env:CONDA_PREFIX
if (-not $condaBase) {
  # Try to find conda in PATH
  $condaExe = Get-Command conda -ErrorAction SilentlyContinue
  if ($condaExe) {
    $condaBase = & $condaExe info --base 2>$null
  }
}

if ($condaBase -and (Test-Path "$condaBase\Library\lib\geos_c.lib")) {
  Write-Host "Found conda GEOS at: $condaBase"
  $env:GEOS_LIB_DIR = "$condaBase\Library\lib"
  $env:GEOS_INCLUDE_DIR = "$condaBase\Library\include"
  $env:Path = "$condaBase\Library\bin;$env:Path"
  # Detect GEOS version from conda
  $geosVer = & $condaExe list geos --json 2>$null | ConvertFrom-Json | Where-Object { $_.name -eq "geos" } | Select-Object -ExpandProperty version
  if ($geosVer) {
    $env:GEOS_VERSION = $geosVer
  }
} else {
  Write-Warning "GEOS not found in conda environment. Install with: conda install -c conda-forge geos"
  Write-Warning "On Linux: sudo apt install libgeos-dev  |  macOS: brew install geos"
  Write-Warning "Then run this script again."
  exit 1
}

$args = @("bench", "--features", $Features, "--bench", $Bench)
if ($Dataset) {
  $args += "--"
  $args += $Dataset
}

Write-Host "Running: cargo $($args -join ' ')"
Write-Host "GEOS_LIB_DIR = $env:GEOS_LIB_DIR"
Write-Host "GEOS_INCLUDE_DIR = $env:GEOS_INCLUDE_DIR"
Write-Host "GEOS_VERSION = $env:GEOS_VERSION"
Write-Host ""

cargo @args

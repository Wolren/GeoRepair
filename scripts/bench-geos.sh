#!/usr/bin/env bash
set -euo pipefail

BENCH="${1:-real_world}"
DATASET="${2:-}"
FEATURES="${FEATURES:-bench-geos,arrange,structure,parallel,simd}"

detect_geos() {
  # conda
  if command -v conda &>/dev/null; then
    local base
    base="$(conda info --base 2>/dev/null)" || true
    if [[ -n "$base" && -f "$base/Library/lib/geos_c.lib" ]]; then
      export GEOS_LIB_DIR="$base/Library/lib"
      export GEOS_INCLUDE_DIR="$base/Library/include"
      export PATH="$base/Library/bin:$PATH"
      local ver
      ver="$(conda list geos 2>/dev/null | awk '/geos/{print $2;exit}')" || true
      [[ -n "$ver" ]] && export GEOS_VERSION="$ver"
      echo "Found conda GEOS at: $base  (version: ${ver:-unknown})"
      return 0
    fi
  fi
  # pkg-config (Linux/macOS)
  if command -v pkg-config &>/dev/null && pkg-config --exists geos 2>/dev/null; then
    echo "Found system GEOS via pkg-config"
    return 0
  fi
  # CMake find_package
  if command -v cmake &>/dev/null; then
    local gd="${GEOS_ROOT:-/usr}"
    if cmake -P /dev/null 2>/dev/null <<<"find_package(GEOS REQUIRED)" ; then
      echo "Found system GEOS via CMake"
      return 0
    fi
  fi
  return 1
}

if ! detect_geos; then
  echo "ERROR: GEOS not found."
  echo "  Windows (conda): conda install -c conda-forge geos"
  echo "  Debian/Ubuntu:   sudo apt install libgeos-dev"
  echo "  macOS:           brew install geos"
  exit 1
fi

echo "GEOS_LIB_DIR=${GEOS_LIB_DIR:-}"
echo "GEOS_INCLUDE_DIR=${GEOS_INCLUDE_DIR:-}"
echo "GEOS_VERSION=${GEOS_VERSION:-}"
echo ""

ARGS=(bench --features "$FEATURES" --bench "$BENCH")
[[ -n "$DATASET" ]] && ARGS+=(-- "$DATASET")

echo "Running: cargo ${ARGS[*]}"
cargo "${ARGS[@]}"

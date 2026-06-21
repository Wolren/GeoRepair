#!/usr/bin/env bash
# Fetch geos-src crate for GEOS comparison benchmarks.
#
# Usage:
#   ./scripts/fetch-geos-src.sh [version]
#
# Default version: 0.2.4
# After fetching, run: cargo bench --features bench-geos
set -euo pipefail

VERSION="${1:-0.2.4}"
TARGET="patches/geos-src"

# Idempotency check — already present
if [ -d "$TARGET" ] && [ -f "$TARGET/.cargo-ok" ]; then
  echo "✓ geos-src v$VERSION already present at $TARGET"
  exit 0
fi

echo "Downloading geos-src v$VERSION from crates.io ..."
mkdir -p patches

TEMPDIR="$(mktemp -d)"
CRATE_FILE="$TEMPDIR/geos-src.crate"

# Download from crates.io CDN
curl -sL "https://static.crates.io/crates/geos-src/geos-src-$VERSION.crate" \
  -o "$CRATE_FILE" 2>/dev/null || {
  # Fallback: crates.io API redirect
  curl -sL "https://crates.io/api/v1/crates/geos-src/$VERSION/download" \
    -o "$CRATE_FILE" 2>/dev/null
}

if [ ! -f "$CRATE_FILE" ] || [ ! -s "$CRATE_FILE" ]; then
  echo "✗ Failed to download geos-src v$VERSION from crates.io"
  echo "  Check your network connection and that the version exists:"
  echo "  https://crates.io/crates/geos-src/$VERSION"
  rm -rf "$TEMPDIR"
  exit 1
fi

# Extract — crates.io archives produce geos-src-$VERSION/
tar xzf "$CRATE_FILE" -C patches/
rm -rf "$TEMPDIR"

# Rename to the canonical patches/geos-src/ path
if [ -d "patches/geos-src-$VERSION" ]; then
  # Remove partial extraction from a previous failed run, if any
  rm -rf "$TARGET"
  mv "patches/geos-src-$VERSION" "$TARGET"
fi

# Create marker so idempotency check works on re-runs
touch "$TARGET/.cargo-ok"

echo ""
echo "✓ geos-src v$VERSION installed at $TARGET"
echo ""
echo "Now run GEOS comparison benchmarks:"
echo "  cargo bench --features bench-geos"
echo ""
echo "Or use the existing helper:"
echo "  ./scripts/bench-geos.sh"

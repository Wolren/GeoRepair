#!/usr/bin/env bash
# Version bump for geo-repair: syncs Cargo.toml, pyproject.toml and the
# CHANGELOG, then regenerates Cargo.lock. Run from the repo root.
#
#   scripts/bump_version.sh 0.14.3
#
# Semver rule (Wolren, 2026-08-03): patch is the safe default; use the
# number the user named, never invent a major/minor bump.
set -euo pipefail

NEW="${1:-}"
if [[ -z "$NEW" || ! "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "usage: $0 <X.Y.Z>" >&2
    exit 1
fi

# Cargo.toml: bump the [package] version line (whole-line replace).
sed -i -E "0,/^version = /s/^version = \".*\"/version = \"$NEW\"/" Cargo.toml
# pyproject.toml: same, but only inside [project] (first occurrence).
sed -i -E "0,/^version = /s/^version = \".*\"/version = \"$NEW\"/" pyproject.toml

# CHANGELOG: rename "Unreleased" to the new version, add a fresh one.
if grep -q "^## \[Unreleased\]" CHANGELOG.md; then
    sed -i "0,/^## \[Unreleased\]/s//## [$NEW] - $(date +%Y-%m-%d)/" CHANGELOG.md
    sed -i "2i\\
\\
## [Unreleased]" CHANGELOG.md
else
    echo "warning: no [Unreleased] section found; add the $NEW section manually" >&2
fi

# Regenerate Cargo.lock for the new version.
cargo update -p geo-repair --precise "$NEW" 2>/dev/null || cargo generate-lockfile

echo "version bumped to $NEW (Cargo.toml + pyproject.toml + CHANGELOG + Cargo.lock)"
echo
echo "RELEASE CHECKLIST (docs/RELEASE.md):"
echo "  1. cargo test --features \"arrange,structure,parallel,simd,validate\""
echo "  2. cargo clippy --features \"arrange,structure,parallel,simd,io-geojson,io-wkt,io-wkb,io-csv,io-gpkg,io-gml,ffi\" -- -D warnings"
echo "  3. cargo deny check licenses advisories sources && cargo audit"
echo "  4. cargo semver-checks check-release"
echo "  5. cargo bench --features \"arrange,structure,parallel,simd,bench-geos-system,io-gpkg\" --bench real_world -- --fast  (full-pass band 3.5-3.9s)"
echo "  6. maturin build --release && pytest tests/test_python.py"
echo "  7. push -> CI green (16 jobs) -> cargo publish -> maturin publish"

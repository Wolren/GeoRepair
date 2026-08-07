# Release Checklist

Every geo-repair release, in order. The whole point is that no step is
skipped and nothing depends on memory: the 0.15.0/0.14.2 incident
(2026-08-03) happened because the version dance was manual.

## 1. Version

```bash
scripts/bump_version.sh X.Y.Z
```

The script syncs `Cargo.toml`, `pyproject.toml`, `CHANGELOG.md` (renames
`[Unreleased]`) and regenerates `Cargo.lock`. Patch is the safe default;
use the number the user named, never invent a major/minor bump.

## 2. Changelog

Fill the new section under Keep a Changelog headings. Fixed entries name
the symptom, the root cause and the verification (measurements, suite
counts). No "misc" entries.

## 3. Local gates

```bash
cargo test --features "arrange,structure,parallel,simd,validate"
cargo test --features "arrange,structure,parallel,simd,ffi" --test ffi
cargo clippy --features "arrange,structure,parallel,simd,io-geojson,io-wkt,io-wkb,io-csv,io-gpkg,io-gml,ffi" -- -D warnings
cargo deny check licenses advisories sources
cargo audit
cargo semver-checks check-release
cargo fuzz build   # nightly; Windows cannot link cdylib, do this on CI/linux
maturin build --release --features python && pip install --force-reinstall target/wheels/*.whl && pytest tests/test_python.py -q
# C harness (compiles tests/c/test_geo_repair.c against the built lib):
cargo build --release --features ffi
#   Linux/macOS: gcc -std=c99 -Wall -Wextra -I include tests/c/test_geo_repair.c \
#       -L target/release -l geo_repair -o target/release/gr_test \
#     && LD_LIBRARY_PATH=target/release target/release/gr_test
#   Windows:     tests\c\build_test_windows.bat   (vcvars via vswhere)
```

Baselines: GEOS XML suite 937/937 dispatched (213 masked divergences
documented, never grow the mask silently), differential fuzz green,
fuzz invariants green.

## 4. Bench (dual bar)

```bash
cargo bench --features "arrange,structure,parallel,simd,bench-geos-system,io-gpkg" --bench real_world -- --fast
```

Runs on the full 1,579,030-part dataset (the bench defaults to
`data_0.gpkg`; do NOT point it at the lossy transcription). Full-pass
band: 3.5-3.9s. A release that moves the bar outside the band without a
documented reason does not ship. The committed `alaska.*` set in CI is a
small sample and is not the bar.

## 5. Push and CI

Push, then watch all 16 jobs to green: check, msrv, no-std, test,
test-no-defaults, python, test-geos-oracle, test-serde, bench, clippy,
rustdoc, semver, audit, deny, fuzz (5 smokes + ASan; cargo-fuzz has no
UBSan), fuzz
artifact upload on failure. A fuzz crash means a corpus seed + a root
cause before release.

## 6. Publish

```bash
cargo publish --dry-run   # verify
cargo publish
maturin publish           # wheels (verify credentials/CI first)
```

docs.rs is preconfigured with an explicit feature list (all-features
would fail on simd-portable nightly and bench-geos-system).

## 7. Post-release

- Confirm docs.rs built (its build is async; check the badge/page).
- Nothing to bump: the semver gate uses `check-release` (crates.io
  latest), no hardcoded baseline.
- If the release fixed fuzz-discovered crashes, the crash artifacts were
  already committed as corpus seeds in step 5 - confirm the seeds are in
  `fuzz/corpus/` and not just left as CI artifacts.

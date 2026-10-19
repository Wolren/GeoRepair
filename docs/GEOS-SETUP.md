# GEOS benchmark setup (Windows x86_64 MSVC)

GEOS is installed via conda at: `D:\Miniconda`

## Required environment variables

```powershell
$env:GEOS_LIB_DIR = "D:\Miniconda\Library\lib"
$env:GEOS_INCLUDE_DIR = "D:\Miniconda\Library\include"
$env:GEOS_VERSION = "3.14.1"
$env:Path = "D:\Miniconda\Library\bin;$env:Path"
```

## Run benchmark with GEOS comparison

Two features control GEOS linking:
- `bench-geos` compiles GEOS from C source (slower GEOS, no LTO; use for CI)
- `bench-geos-system` links against system GEOS through env vars (faster, LLVM-optimized)

```powershell
# System GEOS (faster: uses the conda LLVM build)
$env:GEOS_LIB_DIR = "D:\Miniconda\Library\lib"
$env:GEOS_INCLUDE_DIR = "D:\Miniconda\Library\include"
$env:GEOS_VERSION = "3.14.1"
$env:Path = "D:\Miniconda\Library\bin;$env:Path"
cargo bench --features bench-geos-system,arrange,structure,parallel,simd,io-shp --bench real_world

# Static-built GEOS (slower: MSVC, no LTO)
cargo bench --features bench-geos,arrange,structure,parallel,simd,io-shp --bench real_world
```

## Run tests (without GEOS)

```powershell
# Core library tests
cargo test --features arrange,structure,parallel,simd,validate

# Fuzz tests (256 cases, fast)
PROPTEST_CASES=256 cargo test --features arrange,structure,parallel,simd,validate --test fuzz

# Fuzz tests (5000 cases, thorough)
PROPTEST_CASES=5000 cargo test --features arrange,structure,parallel,simd,validate --test fuzz

# All features (slow: includes IO, benchmarks, Python bindings)
cargo test --all-features

# Clippy (CI gate)
cargo clippy --features arrange,structure,parallel,simd,validate -- -D warnings

# Full CI check
cargo check --all-features
```

## Build and install Python wheel for QGIS

QGIS (OSGeo4W) uses Python at `C:\Users\Wildbot\AppData\Local\Programs\OSGeo4W\apps\Python312\`.

```powershell
# Build wheel
& "C:\Users\Wildbot\AppData\Local\Programs\OSGeo4W\apps\Python312\python.exe" -m maturin build --release --features python

# Install into QGIS Python (force reinstall; the wheel version follows Cargo.toml)
& "C:\Users\Wildbot\AppData\Local\Programs\OSGeo4W\apps\Python312\python.exe" -m pip install target\wheels\geo_repair-*.whl --force-reinstall

# Copy QGIS processing script
Copy-Item -Path qgis\qgis_geo_repair.py -Destination "$env:APPDATA\QGIS\QGIS3\profiles\default\processing\scripts\" -Force
```

Restart QGIS. Geo Repair appears in the Processing Toolbox.

## Known issues

- Incremental build failed once with "multiple different versions of crate `geo_types`". The dependency tree now shows only `geo-types v0.7.19`, so it seems resolved. Skip `cargo clean` to hold incremental rebuilds at about 56s. If the error comes back, run a full `cargo clean`.
- `cargo clean -p geo-repair` reports "Removed 0 files" and does not clean anything. Use full `cargo clean` if needed.
- `load-shp` is now `io-shp`. Use the correct name.
- The `geos` crate feature `static` compiles GEOS from source (slow, no LTO). System GEOS (conda) is faster.
- WKT-based GEOS conversion is too slow for 1.58M polygons. Use CoordSeq direct construction (already implemented in benchmark).

## Cargo features: `dep:rstar` trap

NEVER use `dep:rstar` directly in a feature list. Always use the `rstar` feature
alias defined in Cargo.toml:

```toml
# Correct
foo = ["rstar"]

# Wrong: cfg(feature = "rstar") is NOT set, so the R-tree acceleration stays off
foo = ["dep:rstar"]
```

`dep:rstar` enables the `rstar` crate but does NOT set `cfg(feature = "rstar")`,
causing every `#[cfg(feature = "rstar")]` block to be silently compiled out.
This was a production bug in v0.10.0. See commits `f02456b` → `91863e5`.
A `compile_error!` guard in `src/lib.rs` catches this at build time.

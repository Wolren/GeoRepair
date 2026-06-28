# GEOS benchmark setup (Windows x86_64 MSVC)

GEOS is installed via conda at: `C:\Users\Wildbot\miniconda3`

## Required environment variables

```powershell
$env:GEOS_LIB_DIR = "C:\Users\Wildbot\miniconda3\Library\lib"
$env:GEOS_INCLUDE_DIR = "C:\Users\Wildbot\miniconda3\Library\include"
$env:GEOS_VERSION = "3.14.1"
$env:Path = "C:\Users\Wildbot\miniconda3\Library\bin;$env:Path"
```

## Run benchmark with GEOS comparison

```powershell
# Set env vars, full clean (required due to incremental build bug with cdylib+lib), then run
$env:GEOS_LIB_DIR = "C:\Users\Wildbot\miniconda3\Library\lib"
$env:GEOS_INCLUDE_DIR = "C:\Users\Wildbot\miniconda3\Library\include"
$env:GEOS_VERSION = "3.14.1"
$env:Path = "C:\Users\Wildbot\miniconda3\Library\bin;$env:Path"
cargo clean
cargo bench --features bench-geos,arrange,structure,parallel,simd,load-shp --bench real_world
```

## Run tests (without GEOS)

```powershell
cargo test --features arrange,structure,parallel,simd,load-shp
```

## Known issues

- Incremental build fails with "multiple different versions of crate `geo_types`" when rebuilding. Always `cargo clean` before running the benchmark.
- `cargo clean -p geo-repair` reports "Removed 0 files" and doesn't actually clean. Use full `cargo clean`.
- The `geos` crate feature `static` compiles GEOS from source (slow, no LTO). System GEOS (conda) is faster.
- WKT-based GEOS conversion is too slow for 1.58M polygons. Use CoordSeq direct construction (already implemented in benchmark).

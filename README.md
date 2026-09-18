<div align="center">

# GeoRepair

OGC geometry repair and validation for Rust. Passes the GEOS XML validation suite.

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.88+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/Wolren/GeoRepair?tab=License-1-ov-file)
[![Status](https://img.shields.io/badge/status-experimental-orange.svg)]()
[![Ko-fi](https://img.shields.io/badge/Ko--fi-Support%20Wolren-FF5E5B?logo=ko-fi&logoColor=white)](https://ko-fi.com/wolren)

</div>

> **This crate is experimental.** The API is actively evolving: expect
> breaking changes between 0.x releases. Core algorithms, I/O backends,
> and feature flags are all subject to change as we improve correctness
> and performance.

Detects and fixes invalid GIS geometries (self-intersections, unclosed
rings, degenerate shapes, NaN coordinates) using algorithms selected by
geometry type. Built-in I/O for WKB, WKT, and a custom binary batch
format with no extra dependencies.

The **Structure** strategy (default) mirrors GEOS ST_MakeValid: planar
graph extraction, face walking, winding-number assembly. The **Arrange**
strategy is a CDT-based fallback for complex topologies.

Performance on the 1,579,030-polygon production dataset (i5-12400F,
release, parallel batch, GEOS 3.14.1 conda-forge as reference):

| Dataset | GeoRepair | GEOS | vs GEOS |
|---------|----------:|-----:|:-------:|
| Validation (1.58M) | **3.0-3.2 s** | 3.6-3.9 s | **0.75-0.9x** |
| Invalid subset | 0 polys (2026-08-06) | 0 | - |
| Full dataset (1.58M polys) | 3.8-4.0 s | **3.3-3.5 s** | 1.1-1.2x |

## Performance

### Real-world dataset (1,579,030 polygons)

Structure batch on a production GIS dataset (813 features from a GeoPackage,
flattened to 1,579,030 polygon parts). i5-12400F (6C/12T), release LTO,
Rayon 12-thread batch; GEOS is conda-forge 3.14.1, serial per-call, run
concurrently via Rayon, geometries built via CoordSeq direct construction
(the one-time 1.5 s pre-build is excluded). Ratio is GEOS / GeoRepair:
**>1 means GeoRepair is faster** (same convention as the synthetic tables).

| Dataset | GeoRepair | GEOS | Ratio |
|---------|----------:|-----:|------:|
| Validation (1.58M) | **2.1 s** (1.3 µs/poly) | 3.0 s (1.9 µs/poly) | **1.4x** |
| Full pass (1.58M) | 3.6 s (2.3 µs/poly) | **3.0 s** (1.9 µs/poly) | 0.83x |

Validity agreement with GEOS: 100% (full validator, 0/0 disagreements); the gate screen flags 139 of 1,579,030 that GEOS accepts (99.99%, 2026-09-18).

### Synthetic benchmarks

Full table, parallel batch (µs); ratio = GEOS / GeoRepair, >1 means
we win. Methodology: `docs/BENCHMARKS.md`. Regenerate:
`python scripts/readme_bench_table.py --update <bench.json>`; CI gate:
`python scripts/readme_bench_table.py --check <bench.json>`.

| Benchmark | GeoRepair | GEOS | Ratio |
|-----------|----------:|-----:|------:|
| valid polygon 4v | 0.049 | 0.21 | 4.3x |
| valid polygon 10v | 0.141 | 0.295 | 2.1x |
| valid polygon 50v | 0.326 | 0.376 | 1.2x |
| valid polygon 100v | 0.484 | 0.494 | 1.0x |
| valid polygon 500v | 1.9 | 1.2 | 0.64x |
| valid polygon 1000v | 3.5 | 2.2 | 0.62x |
| valid polygon 5000v | 18.0 | 8.4 | 0.47x |
| valid polygon 10000v | 39.5 | 17.0 | 0.43x |
| invalid bowtie 4v | 0.646 | 16.7 | 26x |
| invalid bowtie 50v | 6.9 | 62.6 | 9.0x |
| invalid bowtie 100v | 23.9 | 123 | 5.2x |
| invalid bowtie 500v | 53.8 | 499 | 9.3x |
| invalid bowtie 1000v | 122 | 1017 | 8.3x |
| star poly 100v | 8.5 | 7.5 | 0.88x |
| star poly 500v | 186 | 87.3 | 0.47x |
| star poly 1000v | 229 | 271 | 1.2x |
| spaghetti 500v | 4975 | 2455 | 0.49x |
| spaghetti 2000v | 32163 | 15747 | 0.49x |
| self-touch 100v | 0.676 | 0.718 | 1.1x |
| self-touch 500v | 2.3 | 1.5 | 0.64x |
| self-touch 1000v | 4.3 | 3.0 | 0.71x |
| collapsed 100v | 19.3 | 113 | 5.8x |
| collapsed 500v | 86.0 | 472 | 5.5x |
| collapsed 1000v | 166 | 917 | 5.5x |
| near-collinear 100v | 64.9 | 165 | 2.5x |
| near-collinear 500v | 727 | 1048 | 1.4x |
| near-collinear 1000v | 2663 | 2596 | 0.97x |
| large coord 1e12 100v | 0.864 | 0.533 | 0.62x |
| large coord 1e12 500v | 10.1 | 1.2 | 0.12x |
| large coord 1e12 1000v | 34.3 | 2.7 | 0.08x |
| valid line | 0.013 | 0.047 | 3.7x |
| zero-length line | 0.007 | 0.267 | 38x |
| valid ls 4v | 0.027 | 0.044 | 1.7x |
| valid ls 10v | 0.061 | 0.049 | 0.80x |
| valid ls 50v | 0.151 | 0.098 | 0.65x |
| valid ls 100v | 0.311 | 0.135 | 0.44x |
| valid ls 500v | 1.3 | 0.61 | 0.47x |
| valid ls 1000v | 2.8 | 2.7 | 0.95x |
| collinear ls 4v | 0.033 | 0.543 | 16x |
| collinear ls 10v | 0.065 | 0.592 | 9.1x |
| collinear ls 50v | 0.229 | 0.682 | 3.0x |
| collinear ls 100v | 0.411 | 0.959 | 2.3x |
| collinear ls 500v | 2.3 | 1.6 | 0.71x |
| collinear ls 1000v | 3.6 | 10.0 | 2.8x |
| zigzag ls 10v | 0.058 | 1.2 | 21x |
| zigzag ls 50v | 0.141 | 4.6 | 32x |
| zigzag ls 100v | 0.254 | 9.2 | 36x |
| zigzag ls 500v | 1.5 | 55.0 | 36x |
| zigzag ls 1000v | 2.7 | 113 | 42x |
| spiral ls 10v | 0.064 | 0.817 | 13x |
| spiral ls 50v | 0.508 | 4.6 | 9.1x |
| spiral ls 100v | 1.2 | 15.0 | 12x |
| spiral ls 500v | 113 | 544 | 4.8x |
| spiral ls 1000v | 346 | 1883 | 5.4x |
| self-int ls 100v | 6.0 | 2.3 | 0.38x |
| self-int ls 500v | 37.3 | 3.7 | 0.10x |
| self-int ls 1000v | 110 | 5.8 | 0.05x |
| dense self ls 10v | 0.064 | 1.2 | 19x |
| dense self ls 50v | 0.149 | 4.9 | 33x |
| dense self ls 100v | 0.315 | 11.5 | 37x |
| dense self ls 500v | 1.6 | 120 | 74x |
| dense self ls 1000v | 2.5 | 339 | 134x |
| duped ls 100v | 0.297 | 0.695 | 2.3x |
| duped ls 500v | 0.963 | 1.4 | 1.4x |
| duped ls 1000v | 2.0 | 4.5 | 2.2x |
| mls 50x3v | 2.9 | 30.6 | 11x |
| mls 250x3v | 14.8 | 130 | 8.8x |
| mls 500x3v | 36.9 | 252 | 6.8x |
| self-int mls 50x4v | 28.0 | 73.4 | 2.6x |
| self-int mls 250x4v | 178 | 418 | 2.3x |
| self-int mls 500x4v | 501 | 1035 | 2.1x |
| star-burst 10sp | 2.3 | 13.3 | 5.9x |
| star-burst 50sp | 0.698 | 248 | 356x |
| star-burst 100sp | 1.2 | 1021 | 869x |
| star-burst 500sp | 7.8 | 34393 | 4428x |
| star-burst 1000sp | 17.4 | 170365 | 9767x |
| collinear ov 10seg | 1.0 | 13.7 | 13x |
| collinear ov 50seg | 7.4 | 56.5 | 7.6x |
| collinear ov 100seg | 12.5 | 99.3 | 7.9x |
| collinear ov 500seg | 52.3 | 517 | 9.9x |
| collinear ov 1000seg | 126 | 1115 | 8.8x |
| x-scale 10v | 1.7 | 18.7 | 11x |
| x-scale 50v | 54.1 | 363 | 6.7x |
| x-scale 100v | 229 | 1500 | 6.5x |
| x-scale 500v | 17799 | 54224 | 3.0x |
| x-scale 1000v | 1625 | 345217 | 212x |
| ringing 100v | 11.0 | 62.3 | 5.7x |
| ringing 500v | 43.9 | 334 | 7.6x |
| ringing 1000v | 120 | 965 | 8.0x |
| hilbert 256v | 34.3 | 172 | 5.0x |
| hilbert 1024v | 294 | 1190 | 4.0x |
| lissajous 200v | 19.4 | 140 | 7.2x |
| lissajous 500v | 45.3 | 390 | 8.6x |
| lissajous 1000v | 89.4 | 734 | 8.2x |
| lissajous 2000v | 188 | 1638 | 8.7x |
| lissajous 5000v | 582 | 5113 | 8.8x |
| lissajous 7:4 500v | 34.8 | 51.2 | 1.5x |
| spoke 10sp | 2.3 | 15.5 | 6.8x |
| spoke 50sp | 1.0 | 297 | 296x |
| spoke 100sp | 1.6 | 1277 | 808x |
| spoke 500sp | 15.0 | 39463 | 2628x |
| spoke 1000sp | 20.3 | 187847 | 9274x |
| star-comb 20sp | 0.189 | 2.5 | 13x |
| star-comb 100sp | 3.0 | 30.6 | 10x |
| star-comb 500sp | 72.9 | 704 | 9.6x |
| star-comb 1000sp | 670 | 2587 | 3.9x |
| hole hier 5h | 1.5 | 1.9 | 1.2x |
| hole hier 20h | 5.4 | 8.5 | 1.6x |
| hole hier 50h | 21.6 | 24.3 | 1.1x |
| hole hier 100h | 42.8 | 55.8 | 1.3x |
| overlap mp 5sh | 1.9 | 445 | 231x |
| overlap mp 20sh | 6.4 | 2372 | 373x |
| overlap mp 50sh | 22.8 | 6534 | 286x |
| overlap mp 100sh | 50.2 | 13409 | 267x |
| dense grid 5x5=25 | 7.3 | 1675 | 229x |
| dense grid 10x10=100 | 46.7 | 11887 | 255x |
| dense grid 20x20=400 | 454 | 96986 | 214x |
| dense grid 30x30=900 | 1936 | 316732 | 164x |
| sliver 100v | 1.6 | 2.4 | 1.5x |
| sliver 500v | 9.1 | 15.8 | 1.7x |
| sliver 1000v | 16.1 | 25.0 | 1.6x |
| arrange valid 4v | 0.058 | 0.217 | 3.8x |
| arrange valid 10v | 0.091 | 0.3 | 3.3x |
| arrange valid 50v | 0.622 | 0.406 | 0.65x |
| arrange valid 100v | 0.878 | 0.541 | 0.62x |
| arrange valid 500v | 4.2 | 1.7 | 0.40x |
| arrange valid 1000v | 9.2 | 3.6 | 0.40x |
| arrange bowtie 4v | 1.1 | 14.8 | 14x |
| arrange bowtie 100v | 32.5 | 102 | 3.2x |
| arrange star 10sp | 2.1 | 12.8 | 6.2x |
| arrange star 50sp | 0.859 | 241 | 281x |
| arrange star 100sp | 1.7 | 1074 | 619x |
| arrange star 500sp | 7.9 | 37583 | 4772x |

### Run benchmarks

```shell
# Real-world + synthetic with GEOS comparison (system GEOS, conda)
GEOS_LIB_DIR='D:\Miniconda\Library\lib' GEOS_INCLUDE_DIR='D:\Miniconda\Library\include' \
GEOS_VERSION=3.14.1 cargo bench --features bench-geos-system,arrange,structure,parallel,simd --bench real_world
GEOS_LIB_DIR='D:\Miniconda\Library\lib' GEOS_INCLUDE_DIR='D:\Miniconda\Library\include' \
GEOS_VERSION=3.14.1 cargo bench --features bench-geos-system,arrange,structure,parallel,simd --bench bench

# CI regression gate (no GEOS; fixed subset vs benches/bench_baseline.json)
python scripts/bench_gate.py
```

Measurement rules: always take the settled second run
(first-run-after-build is inflated ~18%); the real-world table carries the
median of three settled runs; never trust a bench binary you
cannot trace to a source file. Full GEOS setup: `docs/GEOS-SETUP.md`.

## Limitations

1. **GEOS comparison is against conda-forge MSVC GEOS** (serial per-call,
   no LTO, no mimalloc) - a static LLVM-built GEOS would improve the GEOS
   side of every table.
2. **Validator strictness is deliberate:** eps-class predicates (1e-12 x
   scale tolerance, fast-FP-first with exact escalation). On the 1.58M
   real-world dataset it agrees with GEOS 0/0 (winding-agnostic); it
   diverges on borderline inputs (213 baselined XML cases, mostly the OGC
   winding contract - `docs/BENCHMARKS.md`). Repair ships only
   validator-clean geometry and degrades to an empty GeometryCollection
   otherwise.
3. **W/12 pool-saturation floor:** the parallel batch fills all 12 workers
   with giants; nested intra-poly rayon finds no idle threads in-batch
   (Amdahl-bound; standalone the parallel check measures 96 ms vs 53 ms in-batch).
4. **Giants (>4,096 ring edges) route to the boolean pipeline** - on a
   200k-edge giant single-pass noding costs 168 ms vs 36 ms for the
   boolean path (`SP_MAX_EDGES` in `src/core/mod.rs`).
5. **Mass-overlap repairs are the slowest synthetic class** (~0.46 ms/poly
   at dense grid 20x20) but stay ~230x faster than GEOS makeValid on the
   same shapes.
6. **Python bindings: `tests/test_python.py` covers the WKB and WKT surface**
   (41 tests); GeoJSON bindings were removed deliberately.
7. **`simd-portable` is nightly-only** (3 E0554 on stable, expected);
   hand-written AVX2 beyond the bbox scan measured slower than
   auto-vectorized scalar (point_in_ring 8.4x, is_ring_ccw 2.8x), and
   `-C target-cpu=native` regresses the full pass ~25%.
8. **`proj` requires native PROJ and is mutually exclusive with `io-gpkg`**
   (sqlite3 link conflict); `io-gpkg` is a default feature, so `proj` users
   build with `--no-default-features --features proj,...`.
9. **GEOS XML suite:** 937/937 dispatched cases pass; 3,629 overlay/relate
   cases are skipped (out of scope); 213 masked divergences (documented
   tolerance gates). The suite's WKT/WKB readers are our own - external
   `wkt`/`wkb` crates are disallowed because they measure slower.
10. **Sub-µs synthetic rows are noise** (Rayon dispatch overhead); the
    trustworthy metrics are the real-world batch numbers and the larger
    synthetic rows.

## Integration with the geo ecosystem

geo-repair is built on `geo` types and plugs into the georust ecosystem two
ways:

- **geo-traits sources** (`geo-traits` feature). The `interop` module runs
  validation and repair over `geo_traits::GeometryTrait` /
  `geo_traits::GeometryCollectionTrait`, the trait layer implemented by
  `geo`, geoarrow, geozero, and `wkb`. Any such source can be validated or
  repaired in one call (`interop::is_valid_geometry`,
  `interop::make_valid_geometry`, `interop::make_valid_geometries`, ...)
  without materializing `geo` types; results come back as `geo::Geometry<f64>`.

- **geo's `Validation` trait** (always available). `GeoRepairValidation(&geometry)`
  wraps any `&geo::Geometry<f64>` and exposes geo_repair's validator through
  geo's `Validation` trait (`.is_valid()`, `.check_validation()`,
  `.validation_errors()`) with geo's `Invalid*` error taxonomy. The orphan
  rule prevents implementing geo's trait for geo's own types, so the adapter
  is the bridge. Mapping is best-effort: geo_repair's stricter gates (32-ulp
  collinear, T-junction) surface through it, and classes geo does not model
  (ring closure, orientation, duplicates, ...) are omitted from the geo view.

```rust
use geo::algorithm::validation::Validation;
use geo_repair::GeoRepairValidation;

let adapter = GeoRepairValidation(&geometry);
assert!(!adapter.is_valid());
```

## Python bindings

The `geo-repair` PyPI package exposes the full validation and repair
surface over WKB bytes and WKT text, single geometry and batch
(including a rayon-backed parallel batch). abi3 wheels (`cp38-abi3`)
serve Python 3.8+; typing stubs are shipped in the wheel.

```bash
pip install geo-repair
```

```python
import geo_repair

fixed = geo_repair.repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")
assert geo_repair.is_valid_wkt(fixed)

was_valid, errors, fixed_wkb = geo_repair.validate_and_fix_wkb(wkb_bytes)
results = geo_repair.par_repair_wkb_batch(list_of_wkb_bytes)
```

Every function exists for WKB and WKT (`repair_*`, `repair_*_batch`,
`par_repair_*_batch`, `repair_validate_*`, `is_valid_*`, `validate_*`,
`validate_and_fix_*`, plus batch forms), with `method`
(auto/arrange/structure) and `keep_collapsed` parameters. A QGIS
Processing script (`qgis/qgis_geo_repair.py`) streams features through
the WKB batch API. Full API reference: `docs/BINDINGS.md`.

## C API

The `ffi` feature exposes a panic-safe C API over WKB and WKT, single
geometries and parallel batches:

```bash
cargo build --release --features ffi
# output: target/release/geo_repair.{dll,so,dylib} + libgeo_repair.a + include/geo_repair.h
```

```c
#include "geo_repair.h"

GeoRepairResult r = geo_repair_make_valid(bowtie_wkb, bowtie_wkb_len);
if (r.success) { /* r.wkb_data / r.wkb_len = fixed WKB */ }
geo_repair_free_result(&r);
```

Every result carries a `GeoRepairErrorCode` (None/Parse/InvalidInput/
InvalidGeometry/Encode/Panic); batches report per-item outcomes without
failing as a whole. All results must be freed with the matching
`geo_repair_free_*` (double-free safe). The ABI (struct layouts, error
codes) is fixed from 0.14.2; panic containment requires the release
profile's `panic = "unwind"`. Prebuilt libraries for Windows, Linux, and
macOS are attached to every GitHub release. Full API reference:
`docs/BINDINGS.md`.

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `arrange` | CDT-based polygon repair (requires `spade`) | yes |
| `structure` | Structure-based fast path repair | yes |
| `parallel` | Rayon parallel processing (non-WASM) | yes |
| `simd` | No-op on stable (kernels compiled unconditionally, LLVM auto-vectorized; hand-written AVX2 measured slower, kept only for the nightly `simd-portable` path) | no |
| `validate` | OGC validation predicates | yes |
| `mimalloc` | Use mimalloc global allocator | yes |
| `std` | Standard library + file I/O. Disable for no_std builds. | yes |
| `simd-portable` | Portable SIMD via `core::simd` (nightly only) | no |
| `memmap` | Memory-mapped binary file loading | no |
| `wasm` | WASM browser fetch (synchronous XHR) | no |
| `proj` | CRS transformation (placeholder) | no |
| `serde` | Geometry serde support (`geo/serde`) | no |
| `ffi` | C-compatible FFI bindings | no |
| `geo-traits` | Interop surface over `geo_traits::GeometryTrait` / `GeometryCollectionTrait` (geo, geoarrow, geozero, wkb sources) | no |
| `python` | Python bindings via PyO3 | no |
| `io-shp` | Shapefile format backend | no |
| `io-wkt` | No-op (WKT is built-in, kept for CI compatibility) | no |
| `io-csv` | CSV format backend | no |
| `io-gml` | GML/XML format backend | no |
| `io-gpkg` | GeoPackage format backend (default; gated out on wasm32) | yes |
| `io-all` | All opt-in backends except gpkg | no |
| `io-all-native` | All opt-in backends including gpkg | no |
| `bench-geos` | GEOS comparison benchmarks (build from source, MSVC, no LTO) | no |
| `bench-geos-system` | GEOS comparison benchmarks (link against system GEOS, conda-forge MSVC) | no |

## License

Apache-2.0

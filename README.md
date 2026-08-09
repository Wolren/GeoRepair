<div align="center">

# GeoRepair

OGC geometry repair and validation for Rust. Passes the GEOS XML validation suite.

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.85+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)
[![Status](https://img.shields.io/badge/status-experimental-orange.svg)]()

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
| Validation (1.58M) | **2.5 s** (1.6 µs/poly) | 3.5 s (2.2 µs/poly) | **1.4x** |
| Full pass (1.58M) | 4.0 s (2.6 µs/poly) | **3.5 s** (2.2 µs/poly) | 0.87x |

Validity agreement with GEOS: 100% (0/0 disagreements, 2026-08-07).

### Synthetic benchmarks

Full table, parallel batch (µs); ratio = GEOS / GeoRepair, >1 means
we win. Methodology: `docs/BENCHMARKS.md`. Regenerate:
`python scripts/readme_bench_table.py --update <bench.json>`; CI gate:
`python scripts/readme_bench_table.py --check <bench.json>`.

| Benchmark | GeoRepair | GEOS | Ratio |
|-----------|----------:|-----:|------:|
| valid polygon 4v | 0.069 | 0.252 | 3.6x |
| valid polygon 10v | 0.141 | 0.318 | 2.3x |
| valid polygon 50v | 0.349 | 0.421 | 1.2x |
| valid polygon 100v | 0.539 | 0.589 | 1.1x |
| valid polygon 500v | 2.1 | 1.2 | 0.59x |
| valid polygon 1000v | 3.9 | 2.1 | 0.55x |
| valid polygon 5000v | 20.5 | 9.4 | 0.46x |
| valid polygon 10000v | 39.0 | 16.9 | 0.43x |
| invalid bowtie 4v | 0.64 | 17.6 | 27x |
| invalid bowtie 50v | 6.2 | 60.6 | 9.7x |
| invalid bowtie 100v | 24.2 | 117 | 4.8x |
| invalid bowtie 500v | 54.6 | 488 | 8.9x |
| invalid bowtie 1000v | 114 | 967 | 8.5x |
| star poly 100v | 7.5 | 7.1 | 0.94x |
| star poly 500v | 531 | 110 | 0.21x |
| star poly 1000v | 246 | 268 | 1.1x |
| spaghetti 500v | 4693 | 2521 | 0.54x |
| spaghetti 2000v | 32307 | 16113 | 0.50x |
| self-touch 100v | 0.672 | 0.805 | 1.2x |
| self-touch 500v | 2.2 | 1.6 | 0.75x |
| self-touch 1000v | 4.5 | 3.4 | 0.77x |
| collapsed 100v | 19.5 | 119 | 6.1x |
| collapsed 500v | 86.9 | 487 | 5.6x |
| collapsed 1000v | 172 | 921 | 5.3x |
| near-collinear 100v | 63.6 | 166 | 2.6x |
| near-collinear 500v | 755 | 1066 | 1.4x |
| near-collinear 1000v | 2631 | 2622 | 1.00x |
| large coord 1e12 100v | 0.733 | 0.486 | 0.66x |
| large coord 1e12 500v | 8.1 | 1.4 | 0.17x |
| large coord 1e12 1000v | 33.1 | 2.6 | 0.08x |
| valid line | 0.009 | 0.04 | 4.4x |
| zero-length line | 0.007 | 0.252 | 38x |
| valid ls 4v | 0.026 | 0.042 | 1.6x |
| valid ls 10v | 0.056 | 0.05 | 0.88x |
| valid ls 50v | 0.145 | 0.071 | 0.49x |
| valid ls 100v | 0.282 | 0.121 | 0.43x |
| valid ls 500v | 1.2 | 0.536 | 0.45x |
| valid ls 1000v | 2.6 | 2.6 | 0.97x |
| collinear ls 4v | 0.029 | 0.523 | 18x |
| collinear ls 10v | 0.075 | 0.549 | 7.3x |
| collinear ls 50v | 0.237 | 0.66 | 2.8x |
| collinear ls 100v | 0.443 | 0.983 | 2.2x |
| collinear ls 500v | 1.8 | 2.1 | 1.1x |
| collinear ls 1000v | 3.8 | 7.8 | 2.0x |
| zigzag ls 10v | 0.061 | 1.3 | 21x |
| zigzag ls 50v | 0.162 | 4.7 | 29x |
| zigzag ls 100v | 0.296 | 10.6 | 36x |
| zigzag ls 500v | 1.7 | 62.1 | 38x |
| zigzag ls 1000v | 2.9 | 122 | 41x |
| spiral ls 10v | 0.063 | 0.836 | 13x |
| spiral ls 50v | 0.52 | 4.3 | 8.4x |
| spiral ls 100v | 1.3 | 15.7 | 12x |
| spiral ls 500v | 112 | 526 | 4.7x |
| spiral ls 1000v | 384 | 1807 | 4.7x |
| self-int ls 100v | 5.5 | 2.5 | 0.45x |
| self-int ls 500v | 84.2 | 3.9 | 0.05x |
| self-int ls 1000v | 330 | 6.8 | 0.02x |
| dense self ls 10v | 0.058 | 1.3 | 22x |
| dense self ls 50v | 0.155 | 5.4 | 35x |
| dense self ls 100v | 0.304 | 13.5 | 44x |
| dense self ls 500v | 1.6 | 127 | 80x |
| dense self ls 1000v | 2.5 | 361 | 147x |
| duped ls 100v | 0.286 | 0.692 | 2.4x |
| duped ls 500v | 0.936 | 1.6 | 1.7x |
| duped ls 1000v | 2.2 | 3.6 | 1.6x |
| mls 50x3v | 2.8 | 32.3 | 12x |
| mls 250x3v | 15.8 | 132 | 8.4x |
| mls 500x3v | 38.3 | 257 | 6.7x |
| self-int mls 50x4v | 29.7 | 74.4 | 2.5x |
| self-int mls 250x4v | 191 | 435 | 2.3x |
| self-int mls 500x4v | 520 | 1091 | 2.1x |
| star-burst 10sp | 2.5 | 14.5 | 5.7x |
| star-burst 50sp | 0.677 | 258 | 381x |
| star-burst 100sp | 1.3 | 1115 | 867x |
| star-burst 500sp | 7.0 | 35180 | 5038x |
| star-burst 1000sp | 16.8 | 185213 | 11035x |
| collinear ov 10seg | 0.976 | 11.7 | 12x |
| collinear ov 50seg | 6.6 | 55.4 | 8.4x |
| collinear ov 100seg | 12.7 | 104 | 8.2x |
| collinear ov 500seg | 52.7 | 534 | 10x |
| collinear ov 1000seg | 112 | 1094 | 9.7x |
| x-scale 10v | 1.7 | 20.0 | 12x |
| x-scale 50v | 49.3 | 376 | 7.6x |
| x-scale 100v | 234 | 1541 | 6.6x |
| x-scale 500v | 16886 | 53173 | 3.1x |
| x-scale 1000v | 1719 | 320188 | 186x |
| ringing 100v | 11.7 | 60.8 | 5.2x |
| ringing 500v | 44.1 | 344 | 7.8x |
| ringing 1000v | 123 | 1009 | 8.2x |
| hilbert 256v | 35.0 | 165 | 4.7x |
| hilbert 1024v | 308 | 1212 | 3.9x |
| lissajous 200v | 19.6 | 142 | 7.2x |
| lissajous 500v | 42.4 | 382 | 9.0x |
| lissajous 1000v | 79.4 | 723 | 9.1x |
| lissajous 2000v | 170 | 1583 | 9.3x |
| lissajous 5000v | 504 | 4929 | 9.8x |
| lissajous 7:4 500v | 33.6 | 46.8 | 1.4x |
| spoke 10sp | 2.4 | 14.0 | 5.8x |
| spoke 50sp | 0.774 | 266 | 343x |
| spoke 100sp | 1.3 | 1102 | 816x |
| spoke 500sp | 8.6 | 36076 | 4194x |
| spoke 1000sp | 20.7 | 188121 | 9095x |
| star-comb 20sp | 0.195 | 2.6 | 13x |
| star-comb 100sp | 2.9 | 32.5 | 11x |
| star-comb 500sp | 73.9 | 672 | 9.1x |
| star-comb 1000sp | 698 | 2694 | 3.9x |
| hole hier 5h | 1.5 | 1.9 | 1.2x |
| hole hier 20h | 5.5 | 8.4 | 1.5x |
| hole hier 50h | 21.9 | 22.2 | 1.0x |
| hole hier 100h | 45.6 | 52.2 | 1.1x |
| overlap mp 5sh | 1.8 | 408 | 224x |
| overlap mp 20sh | 6.8 | 2428 | 356x |
| overlap mp 50sh | 21.3 | 6635 | 311x |
| overlap mp 100sh | 49.4 | 13849 | 280x |
| dense grid 5x5=25 | 8.0 | 1655 | 206x |
| dense grid 10x10=100 | 42.1 | 11513 | 273x |
| dense grid 20x20=400 | 459 | 99563 | 217x |
| dense grid 30x30=900 | 1899 | 361528 | 190x |
| sliver 100v | 1.5 | 2.2 | 1.4x |
| sliver 500v | 11.1 | 14.4 | 1.3x |
| sliver 1000v | 16.1 | 25.9 | 1.6x |
| arrange valid 4v | 0.059 | 0.258 | 4.4x |
| arrange valid 10v | 0.1 | 0.333 | 3.3x |
| arrange valid 50v | 0.689 | 0.462 | 0.67x |
| arrange valid 100v | 0.902 | 0.539 | 0.60x |
| arrange valid 500v | 3.7 | 1.4 | 0.38x |
| arrange valid 1000v | 8.0 | 4.0 | 0.50x |
| arrange bowtie 4v | 1.2 | 14.9 | 13x |
| arrange bowtie 100v | 32.3 | 101 | 3.1x |
| arrange star 10sp | 2.2 | 13.3 | 6.0x |
| arrange star 50sp | 1.1 | 251 | 234x |
| arrange star 100sp | 1.4 | 1040 | 733x |
| arrange star 500sp | 7.1 | 36466 | 5126x |

### Run benchmarks
### Run benchmarks
### Run benchmarks
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
(first-run-after-build is inflated ~18%); never trust a bench binary you
cannot trace to a source file. Full GEOS setup: `benches/AGENTS.md`.

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
6. **Python bindings: `tests/test_python.py` covers the WKT surface**
   (18 tests); GeoJSON bindings were removed deliberately.
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
| `simd` | Runtime-dispatched AVX2 bbox kernel (the one SIMD kernel that beat auto-vectorized scalar, 4.5x); all other kernels auto-vectorized scalar | yes |
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

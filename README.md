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

Measured against GEOS on the 1,579,030-polygon production dataset
(i5-12400F, release, parallel batch, GEOS 3.14.1 conda-forge as
reference). Current timings: [Performance](#performance).

## Performance

### Real-world dataset (1,579,030 polygons)

Structure batch on a production GIS dataset (813 features from a GeoPackage,
flattened to 1,579,030 polygon parts). i5-12400F (6C/12T), release LTO,
Rayon 12-thread batch; GEOS is conda-forge 3.14.1, serial per-call, run
concurrently via Rayon, geometries built via CoordSeq direct construction
(the one-time 1.5 s pre-build is excluded).

| Dataset | GeoRepair | GEOS | Ratio |
|---------|----------:|-----:|------:|
| Validation (1.58M) | **2.1 s** (1.3 µs/poly) | 3.0 s (1.9 µs/poly) | **1.4x** |
| Full pass (1.58M) | 3.6 s (2.3 µs/poly) | **3.0 s** (1.9 µs/poly) | 0.83x |

### Synthetic benchmarks

Full table, parallel batch (µs); ratio = GEOS / GeoRepair, >1 means
we win. Methodology: `docs/BENCHMARKS.md`. Regenerate:
`python scripts/readme_bench_table.py --update <bench.json>`; CI gate:
`python scripts/readme_bench_table.py --check <bench.json>`.

| Benchmark | GeoRepair | GEOS | Ratio |
|-----------|----------:|-----:|------:|
| valid polygon 4v | 0.049 | 0.235 | 4.8x |
| valid polygon 10v | 0.149 | 0.322 | 2.2x |
| valid polygon 50v | 0.37 | 0.515 | 1.4x |
| valid polygon 100v | 0.714 | 0.614 | 0.86x |
| valid polygon 500v | 2.3 | 1.9 | 0.81x |
| valid polygon 1000v | 3.9 | 2.2 | 0.57x |
| valid polygon 5000v | 22.0 | 9.4 | 0.43x |
| valid polygon 10000v | 47.7 | 17.7 | 0.37x |
| invalid bowtie 4v | 0.68 | 17.2 | 25x |
| invalid bowtie 50v | 6.6 | 63.5 | 9.6x |
| invalid bowtie 100v | 23.3 | 112 | 4.8x |
| invalid bowtie 500v | 60.1 | 465 | 7.7x |
| invalid bowtie 1000v | 115 | 913 | 8.0x |
| star poly 100v | 7.7 | 6.4 | 0.83x |
| star poly 500v | 164 | 81.0 | 0.49x |
| star poly 1000v | 236 | 269 | 1.1x |
| spaghetti 500v | 4457 | 2494 | 0.56x |
| spaghetti 2000v | 24940 | 16540 | 0.66x |
| self-touch 100v | 0.69 | 0.714 | 1.0x |
| self-touch 500v | 2.2 | 1.6 | 0.71x |
| self-touch 1000v | 3.9 | 3.5 | 0.90x |
| collapsed 100v | 20.1 | 117 | 5.9x |
| collapsed 500v | 90.8 | 487 | 5.4x |
| collapsed 1000v | 166 | 916 | 5.5x |
| near-collinear 100v | 61.4 | 180 | 2.9x |
| near-collinear 500v | 814 | 1115 | 1.4x |
| near-collinear 1000v | 2544 | 2627 | 1.0x |
| large coord 1e12 100v | 0.695 | 0.44 | 0.63x |
| large coord 1e12 500v | 8.5 | 1.2 | 0.14x |
| large coord 1e12 1000v | 30.0 | 2.5 | 0.08x |
| valid line | 0.009 | 0.038 | 4.3x |
| zero-length line | 0.006 | 0.267 | 45x |
| valid ls 4v | 0.028 | 0.038 | 1.4x |
| valid ls 10v | 0.059 | 0.042 | 0.72x |
| valid ls 50v | 0.177 | 0.083 | 0.47x |
| valid ls 100v | 0.337 | 0.118 | 0.35x |
| valid ls 500v | 1.2 | 0.61 | 0.52x |
| valid ls 1000v | 2.4 | 2.6 | 1.1x |
| collinear ls 4v | 0.028 | 0.538 | 20x |
| collinear ls 10v | 0.068 | 0.578 | 8.5x |
| collinear ls 50v | 0.283 | 0.632 | 2.2x |
| collinear ls 100v | 0.415 | 0.886 | 2.1x |
| collinear ls 500v | 2.4 | 1.9 | 0.79x |
| collinear ls 1000v | 3.7 | 20.0 | 5.4x |
| zigzag ls 10v | 0.071 | 1.3 | 19x |
| zigzag ls 50v | 0.216 | 5.0 | 23x |
| zigzag ls 100v | 0.268 | 9.7 | 36x |
| zigzag ls 500v | 1.4 | 61.2 | 43x |
| zigzag ls 1000v | 2.8 | 120 | 43x |
| spiral ls 10v | 0.058 | 0.886 | 15x |
| spiral ls 50v | 0.553 | 5.1 | 9.3x |
| spiral ls 100v | 1.5 | 18.1 | 12x |
| spiral ls 500v | 127 | 563 | 4.4x |
| spiral ls 1000v | 362 | 1933 | 5.3x |
| self-int ls 100v | 6.3 | 2.3 | 0.36x |
| self-int ls 500v | 43.5 | 3.3 | 0.08x |
| self-int ls 1000v | 121 | 6.4 | 0.05x |
| dense self ls 10v | 0.074 | 1.3 | 17x |
| dense self ls 50v | 0.231 | 5.2 | 22x |
| dense self ls 100v | 0.288 | 12.5 | 43x |
| dense self ls 500v | 1.3 | 129 | 102x |
| dense self ls 1000v | 2.9 | 341 | 120x |
| duped ls 100v | 0.365 | 0.715 | 2.0x |
| duped ls 500v | 1.1 | 1.4 | 1.3x |
| duped ls 1000v | 2.2 | 4.2 | 1.9x |
| mls 50x3v | 2.8 | 30.8 | 11x |
| mls 250x3v | 15.5 | 129 | 8.3x |
| mls 500x3v | 39.6 | 276 | 7.0x |
| self-int mls 50x4v | 31.0 | 78.1 | 2.5x |
| self-int mls 250x4v | 193 | 521 | 2.7x |
| self-int mls 500x4v | 619 | 1206 | 1.9x |
| star-burst 10sp | 3.0 | 18.1 | 5.9x |
| star-burst 50sp | 1.0 | 349 | 334x |
| star-burst 100sp | 1.6 | 1438 | 873x |
| star-burst 500sp | 8.5 | 47694 | 5632x |
| star-burst 1000sp | 18.0 | 226317 | 12573x |
| collinear ov 10seg | 1.1 | 13.2 | 12x |
| collinear ov 50seg | 7.1 | 60.9 | 8.5x |
| collinear ov 100seg | 14.3 | 119 | 8.3x |
| collinear ov 500seg | 62.8 | 592 | 9.4x |
| collinear ov 1000seg | 148 | 1284 | 8.7x |
| x-scale 10v | 1.9 | 21.3 | 11x |
| x-scale 50v | 61.6 | 414 | 6.7x |
| x-scale 100v | 263 | 1784 | 6.8x |
| x-scale 500v | 18884 | 60152 | 3.2x |
| x-scale 1000v | 2257 | 357687 | 159x |
| ringing 100v | 10.8 | 62.9 | 5.8x |
| ringing 500v | 56.5 | 348 | 6.2x |
| ringing 1000v | 118 | 962 | 8.1x |
| hilbert 256v | 34.0 | 168 | 4.9x |
| hilbert 1024v | 291 | 1298 | 4.5x |
| lissajous 200v | 19.2 | 144 | 7.5x |
| lissajous 500v | 46.3 | 386 | 8.3x |
| lissajous 1000v | 85.8 | 745 | 8.7x |
| lissajous 2000v | 184 | 1672 | 9.1x |
| lissajous 5000v | 547 | 5204 | 9.5x |
| lissajous 7:4 500v | 33.8 | 46.8 | 1.4x |
| spoke 10sp | 2.5 | 15.0 | 5.9x |
| spoke 50sp | 0.853 | 273 | 320x |
| spoke 100sp | 1.5 | 1161 | 795x |
| spoke 500sp | 9.8 | 46434 | 4761x |
| spoke 1000sp | 23.1 | 214932 | 9300x |
| star-comb 20sp | 0.199 | 2.6 | 13x |
| star-comb 100sp | 3.4 | 34.2 | 9.9x |
| star-comb 500sp | 76.1 | 743 | 9.8x |
| star-comb 1000sp | 703 | 2660 | 3.8x |
| hole hier 5h | 1.6 | 2.0 | 1.2x |
| hole hier 20h | 5.3 | 8.1 | 1.5x |
| hole hier 50h | 19.7 | 25.3 | 1.3x |
| hole hier 100h | 44.6 | 57.4 | 1.3x |
| overlap mp 5sh | 2.2 | 404 | 186x |
| overlap mp 20sh | 10.4 | 2469 | 238x |
| overlap mp 50sh | 21.1 | 6693 | 318x |
| overlap mp 100sh | 49.1 | 14405 | 293x |
| dense grid 5x5=25 | 9.0 | 1662 | 185x |
| dense grid 10x10=100 | 48.5 | 12137 | 250x |
| dense grid 20x20=400 | 461 | 100233 | 217x |
| dense grid 30x30=900 | 1925 | 352380 | 183x |
| sliver 100v | 1.6 | 2.5 | 1.6x |
| sliver 500v | 8.9 | 23.2 | 2.6x |
| sliver 1000v | 18.8 | 30.4 | 1.6x |
| arrange valid 4v | 0.077 | 0.244 | 3.2x |
| arrange valid 10v | 0.1 | 0.332 | 3.3x |
| arrange valid 50v | 0.752 | 0.459 | 0.61x |
| arrange valid 100v | 1.1 | 0.947 | 0.88x |
| arrange valid 500v | 6.5 | 1.6 | 0.25x |
| arrange valid 1000v | 8.4 | 3.3 | 0.39x |
| arrange bowtie 4v | 1.4 | 15.9 | 11x |
| arrange bowtie 100v | 36.1 | 115 | 3.2x |
| arrange star 10sp | 2.4 | 14.0 | 5.8x |
| arrange star 50sp | 0.826 | 284 | 344x |
| arrange star 100sp | 2.3 | 1162 | 497x |
| arrange star 500sp | 7.7 | 42753 | 5560x |

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
   at dense grid 20x20) but stay over 200x faster than GEOS makeValid on the
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

## Integration with the geo ecosystem

geo-repair plugs into georust two ways: a `geo-traits` interop module that
validates or repairs any `GeometryTrait` source (geo, geoarrow, geozero,
`wkb`) in one call, and a `GeoRepairValidation` adapter that exposes our
validator through geo's own `Validation` trait. Details and examples:
[`docs/INTEROP.md`](docs/INTEROP.md).

## Python bindings

The `geo-repair` PyPI package exposes the full validation and repair
surface over WKB bytes and WKT text, single geometry and batch
(including a rayon-backed parallel batch). Install with
`pip install geo-repair`; abi3 wheels (`cp38-abi3`) serve Python 3.8+
and typing stubs ship in the wheel.

Every function exists for WKB and WKT (`repair_*`, `repair_*_batch`,
`par_repair_*_batch`, `repair_validate_*`, `is_valid_*`, `validate_*`,
`validate_and_fix_*`, plus batch forms), with `method`
(auto/arrange/structure) and `keep_collapsed` parameters. A QGIS
Processing script (`qgis/qgis_geo_repair.py`) streams features through
the WKB batch API. Examples and full API reference: `docs/BINDINGS.md`.

## C API

The `ffi` feature exposes a panic-safe C API over WKB and WKT, single
geometries and parallel batches. Build with
`cargo build --release --features ffi`; the output is
`target/release/geo_repair.{dll,so,dylib}` plus `libgeo_repair.a` and
the `include/geo_repair.h` header.

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
| `rstar` | R-tree index used by noding (enabled transitively by `arrange`/`structure`/`validate`) | yes (transitive) |
| `mimalloc` | Use mimalloc global allocator | yes |
| `std` | Standard library + file I/O. Disable for no_std builds. | yes |
| `simd-portable` | Portable SIMD via `core::simd` (nightly only) | no |
| `memmap` | Memory-mapped binary file loading | no |
| `wasm` | WASM browser fetch (synchronous XHR) | no |
| `proj` | CRS transformation via `geo/proj` (requires native PROJ) | no |
| `serde` | Geometry serde support (`geo/serde`) | no |
| `ffi` | C-compatible FFI bindings | no |
| `geo-traits` | Interop surface over `geo_traits::GeometryTrait` / `GeometryCollectionTrait` (geo, geoarrow, geozero, wkb sources) | no |
| `python` | Python bindings via PyO3 | no |
| `io-shp` | Shapefile format backend | no |
| `io-wkt` | No-op (WKT is built-in, kept for CI compatibility) | no |
| `io-wkb` | No-op (WKB is built-in, kept for CI compatibility) | no |
| `io-geojson` | No-op (GeoJSON backend removed, kept for CI compatibility) | no |
| `io-csv` | CSV format backend | no |
| `io-gml` | GML/XML format backend | no |
| `io-gpkg` | GeoPackage format backend (default; gated out on wasm32) | yes |
| `io-all` | All opt-in backends except gpkg | no |
| `io-all-native` | All opt-in backends including gpkg | no |
| `hotpath` | Dev-only: `hotpath::measure` instrumentation on hot paths | no |
| `hotpath-alloc` | Dev-only: `hotpath` instrumentation plus allocation counters | no |
| `bench-geos` | GEOS comparison benchmarks (build from source, MSVC, no LTO) | no |
| `bench-geos-system` | GEOS comparison benchmarks (link against system GEOS, conda-forge MSVC) | no |
| `bench-gdal-system` | GDAL/OGR I/O comparison benchmark (link against system GDAL) | no |

## License

Apache-2.0

<div align="center">

# GeoRepair

OGC compliant geometry repair and validation for Rust.

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

GeoRepair repairs invalid OGC geometries and answers whether a geometry is
valid. It fixes the common damage: self-intersections, unclosed rings,
degenerate shapes, and NaN coordinates. Input that is already valid comes
back unchanged.

Two strategies, chosen by geometry type:

- **Structure** (default) mirrors GEOS `ST_MakeValid`: planar graph
  extraction, face walking, winding-number assembly.
- **Arrange** is a CDT-based fallback for complex topologies.

WKB and WKT reading and writing, plus a custom binary batch format, are
built in with no extra dependencies.

Everything is evaluated against a huge ported test sets from GEOS, CGAL and own fuzz tests. 
The result should always come as valid per GEOS, but the strategy differences might make the output slightly differ.

## Performance

### A test dataset (1,579,030 polygons)

Structure batch on an a GIS dataset (813 features from a GeoPackage,
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
| valid polygon 4v | 0.052 | 0.206 | 3.9x |
| valid polygon 10v | 0.132 | 0.365 | 2.8x |
| valid polygon 50v | 0.27 | 0.379 | 1.4x |
| valid polygon 100v | 0.417 | 0.474 | 1.1x |
| valid polygon 500v | 1.5 | 1.5 | 1.0x |
| valid polygon 1000v | 3.0 | 2.5 | 0.84x |
| valid polygon 5000v | 13.5 | 13.2 | 0.98x |
| valid polygon 10000v | 26.5 | 20.7 | 0.78x |
| invalid bowtie 4v | 1.4 | 24.7 | 18x |
| invalid bowtie 50v | 7.4 | 79.2 | 11x |
| invalid bowtie 100v | 28.6 | 136 | 4.7x |
| invalid bowtie 500v | 58.7 | 504 | 8.6x |
| invalid bowtie 1000v | 118 | 1050 | 8.9x |
| star poly 100v | 12.1 | 10.6 | 0.88x |
| star poly 500v | 230 | 104 | 0.45x |
| star poly 1000v | 242 | 318 | 1.3x |
| spaghetti 500v | 1884 | 3547 | 1.9x |
| spaghetti 2000v | 13963 | 19876 | 1.4x |
| self-touch 100v | 0.492 | 0.721 | 1.5x |
| self-touch 500v | 1.7 | 1.9 | 1.1x |
| self-touch 1000v | 3.0 | 3.4 | 1.1x |
| collapsed 100v | 22.9 | 145 | 6.3x |
| collapsed 500v | 94.5 | 574 | 6.1x |
| collapsed 1000v | 184 | 1092 | 5.9x |
| near-collinear 100v | 71.1 | 220 | 3.1x |
| near-collinear 500v | 925 | 1474 | 1.6x |
| near-collinear 1000v | 3322 | 3330 | 1.0x |
| large coord 1e12 100v | 0.502 | 0.509 | 1.0x |
| large coord 1e12 500v | 1.4 | 1.3 | 0.95x |
| large coord 1e12 1000v | 3.2 | 2.8 | 0.89x |
| valid line | 0.007 | 0.042 | 6.1x |
| zero-length line | 0.011 | 0.3 | 28x |
| valid ls 4v | 0.024 | 0.045 | 1.8x |
| valid ls 10v | 0.055 | 0.057 | 1.0x |
| valid ls 50v | 0.098 | 0.103 | 1.1x |
| valid ls 100v | 0.185 | 0.142 | 0.77x |
| valid ls 500v | 0.919 | 0.57 | 0.62x |
| valid ls 1000v | 2.0 | 2.8 | 1.4x |
| collinear ls 4v | 0.025 | 0.55 | 22x |
| collinear ls 10v | 0.047 | 0.629 | 13x |
| collinear ls 50v | 0.123 | 0.772 | 6.3x |
| collinear ls 100v | 0.169 | 0.795 | 4.7x |
| collinear ls 500v | 0.527 | 2.7 | 5.2x |
| collinear ls 1000v | 1.0 | 9.7 | 9.6x |
| zigzag ls 10v | 0.044 | 1.3 | 30x |
| zigzag ls 50v | 0.076 | 5.6 | 73x |
| zigzag ls 100v | 0.153 | 11.7 | 77x |
| zigzag ls 500v | 0.491 | 60.0 | 122x |
| zigzag ls 1000v | 1.1 | 121 | 115x |
| spiral ls 10v | 0.111 | 0.916 | 8.2x |
| spiral ls 50v | 0.653 | 5.2 | 8.0x |
| spiral ls 100v | 1.6 | 16.6 | 10x |
| spiral ls 500v | 84.4 | 619 | 7.3x |
| spiral ls 1000v | 291 | 2141 | 7.4x |
| self-int ls 100v | 1.3 | 2.3 | 1.7x |
| self-int ls 500v | 4.8 | 4.4 | 0.92x |
| self-int ls 1000v | 10.7 | 9.4 | 0.88x |
| dense self ls 10v | 0.055 | 1.4 | 26x |
| dense self ls 50v | 0.092 | 5.7 | 62x |
| dense self ls 100v | 0.16 | 14.9 | 93x |
| dense self ls 500v | 0.742 | 142 | 191x |
| dense self ls 1000v | 1.1 | 396 | 374x |
| duped ls 100v | 0.255 | 0.84 | 3.3x |
| duped ls 500v | 0.528 | 1.5 | 2.9x |
| duped ls 1000v | 1.1 | 3.6 | 3.2x |
| mls 50x3v | 3.2 | 34.8 | 11x |
| mls 250x3v | 16.7 | 149 | 8.9x |
| mls 500x3v | 39.8 | 299 | 7.5x |
| self-int mls 50x4v | 18.2 | 86.4 | 4.8x |
| self-int mls 250x4v | 156 | 535 | 3.4x |
| self-int mls 500x4v | 473 | 1190 | 2.5x |
| star-burst 10sp | 3.8 | 16.0 | 4.2x |
| star-burst 50sp | 0.741 | 286 | 385x |
| star-burst 100sp | 2.2 | 1291 | 588x |
| star-burst 500sp | 11.5 | 40195 | 3506x |
| star-burst 1000sp | 26.3 | 197021 | 7487x |
| collinear ov 10seg | 1.1 | 14.5 | 14x |
| collinear ov 50seg | 7.0 | 68.3 | 9.8x |
| collinear ov 100seg | 15.0 | 140 | 9.3x |
| collinear ov 500seg | 61.5 | 636 | 10x |
| collinear ov 1000seg | 136 | 1278 | 9.4x |
| x-scale 10v | 2.5 | 25.2 | 10x |
| x-scale 50v | 80.7 | 451 | 5.6x |
| x-scale 100v | 357 | 1948 | 5.5x |
| x-scale 500v | 28675 | 70186 | 2.4x |
| x-scale 1000v | 2426 | 354741 | 146x |
| ringing 100v | 7.7 | 70.4 | 9.1x |
| ringing 500v | 33.4 | 334 | 10x |
| ringing 1000v | 67.1 | 1036 | 15x |
| hilbert 256v | 40.9 | 189 | 4.6x |
| hilbert 1024v | 227 | 1297 | 5.7x |
| lissajous 200v | 24.1 | 164 | 6.8x |
| lissajous 500v | 58.7 | 408 | 7.0x |
| lissajous 1000v | 106 | 783 | 7.4x |
| lissajous 2000v | 239 | 1726 | 7.2x |
| lissajous 5000v | 605 | 5546 | 9.2x |
| lissajous 7:4 500v | 46.0 | 51.6 | 1.1x |
| spoke 10sp | 3.2 | 15.7 | 4.9x |
| spoke 50sp | 0.92 | 268 | 292x |
| spoke 100sp | 1.6 | 1173 | 734x |
| spoke 500sp | 11.8 | 39791 | 3371x |
| spoke 1000sp | 21.4 | 195783 | 9150x |
| star-comb 20sp | 0.441 | 3.0 | 6.7x |
| star-comb 100sp | 6.2 | 40.3 | 6.5x |
| star-comb 500sp | 82.8 | 762 | 9.2x |
| star-comb 1000sp | 720 | 2774 | 3.9x |
| hole hier 5h | 1.4 | 2.0 | 1.4x |
| hole hier 20h | 5.5 | 8.6 | 1.6x |
| hole hier 50h | 21.6 | 22.8 | 1.1x |
| hole hier 100h | 51.9 | 49.6 | 0.96x |
| overlap mp 5sh | 2.1 | 462 | 217x |
| overlap mp 20sh | 11.0 | 2657 | 241x |
| overlap mp 50sh | 28.6 | 7303 | 255x |
| overlap mp 100sh | 77.8 | 15603 | 201x |
| dense grid 5x5=25 | 8.6 | 1864 | 218x |
| dense grid 10x10=100 | 61.6 | 13379 | 217x |
| dense grid 20x20=400 | 566 | 123066 | 217x |
| dense grid 30x30=900 | 2566 | 391344 | 152x |
| sliver 100v | 1.8 | 3.1 | 1.7x |
| sliver 500v | 7.4 | 19.3 | 2.6x |
| sliver 1000v | 18.0 | 30.7 | 1.7x |
| arrange valid 4v | 0.07 | 0.314 | 4.5x |
| arrange valid 10v | 0.136 | 0.451 | 3.3x |
| arrange valid 50v | 0.716 | 0.511 | 0.71x |
| arrange valid 100v | 1.0 | 0.772 | 0.75x |
| arrange valid 500v | 4.5 | 5.3 | 1.2x |
| arrange valid 1000v | 11.3 | 5.8 | 0.51x |
| arrange bowtie 4v | 1.1 | 20.3 | 18x |
| arrange bowtie 100v | 28.4 | 130 | 4.6x |
| arrange star 10sp | 3.2 | 16.8 | 5.3x |
| arrange star 50sp | 1.2 | 262 | 217x |
| arrange star 100sp | 1.9 | 1258 | 666x |
| arrange star 500sp | 8.8 | 40629 | 4637x |

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
the WKB batch API. Mind  that due to overhead it might suffer from performance issues.
Examples and full API reference: `docs/BINDINGS.md`.

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

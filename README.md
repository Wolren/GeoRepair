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

- **Structure** (default) follows the JTS `MakeValid` design: planar graph
  extraction, face walking, even-odd assembly.
- **Arrange** is a CDT-based fallback for complex topologies.

WKB and WKT reading and writing, plus a custom binary batch format, are
built in with no extra dependencies.

WKT keeps Z and M when you ask for it: `read_wkt_zm` returns the geometry, its
`Dims` and the extra ordinates in flat coordinate order (the same layout as
`read_ewkb`), and `write_wkt_zm` writes the modifier back. `read_wkt` reads the
same files and returns x/y, so a Z-enabled layer from OGR or QGIS loads without
error.

Everything is evaluated against the JTS testxml suite, the GEOS XML corpus, a CGAL case set,
plus the crate's own fuzz tests. The result is always valid, but the strategy differences can
make the output slightly differ.

## Performance

### A test dataset (1,579,028 polygons)

Structure batch on a GIS dataset (813 features from a GeoPackage,
flattened to 1,579,028 polygon parts), measured 2026-11-17. i5-12400F
(6C/12T), release LTO, Rayon 12-thread batch; GEOS is conda-forge 3.14.1,
serial per-call, run concurrently via Rayon, geometries built via CoordSeq
direct construction (the one-time 1.5 s pre-build is excluded).

| Dataset | GeoRepair | GEOS | Ratio |
|---------|----------:|-----:|------:|
| Validation (1.58M) | **2.18 s** (1.38 µs/poly) | 3.60 s (2.28 µs/poly) | **1.7x** |
| Full pass (1.58M) | **2.59 s** (1.64 µs/poly) | 3.19 s (2.02 µs/poly) | **1.2x** |

### Synthetic benchmarks

Full table, parallel batch (µs); ratio = GEOS / GeoRepair, >1 means
we win. Measured 2026-11-17 on i5-12400F. The `self-int ls` rows were
re-run in isolation to settle one full-pass outlier. Methodology:
`docs/BENCHMARKS.md`. Regenerate:
`python scripts/readme_bench_table.py --update <bench.json>`; CI gate:
`python scripts/readme_bench_table.py --check <bench.json>`.

| Benchmark | GeoRepair | GEOS | Ratio |
|-----------|----------:|-----:|------:|
| valid polygon 4v | 0.059 | 0.277 | 4.7x |
| valid polygon 10v | 0.099 | 0.343 | 3.5x |
| valid polygon 50v | 0.43 | 0.46 | 1.1x |
| valid polygon 100v | 0.598 | 0.602 | 1.0x |
| valid polygon 500v | 1.8 | 1.3 | 0.75x |
| valid polygon 1000v | 3.6 | 2.1 | 0.59x |
| valid polygon 5000v | 15.7 | 10.0 | 0.63x |
| valid polygon 10000v | 33.1 | 18.0 | 0.54x |
| invalid bowtie 4v | 0.961 | 21.9 | 23x |
| invalid bowtie 50v | 7.0 | 74.1 | 11x |
| invalid bowtie 100v | 23.4 | 121 | 5.2x |
| invalid bowtie 500v | 37.7 | 567 | 15x |
| invalid bowtie 1000v | 108 | 1166 | 11x |
| star poly 100v | 8.4 | 9.3 | 1.1x |
| star poly 500v | 155 | 87.7 | 0.56x |
| star poly 1000v | 286 | 346 | 1.2x |
| spaghetti 500v | 76.5 | 3278 | 43x |
| spaghetti 2000v | 468 | 20147 | 43x |
| self-touch 100v | 0.666 | 0.902 | 1.4x |
| self-touch 500v | 2.1 | 1.6 | 0.79x |
| self-touch 1000v | 4.2 | 3.8 | 0.89x |
| collapsed 100v | 19.0 | 134 | 7.1x |
| collapsed 500v | 41.0 | 581 | 14x |
| collapsed 1000v | 81.5 | 1161 | 14x |
| near-collinear 100v | 57.4 | 205 | 3.6x |
| near-collinear 500v | 745 | 1233 | 1.7x |
| near-collinear 1000v | 2743 | 2931 | 1.1x |
| large coord 1e12 100v | 0.439 | 0.505 | 1.2x |
| large coord 1e12 500v | 2.0 | 2.0 | 1.0x |
| large coord 1e12 1000v | 3.6 | 4.1 | 1.1x |
| valid line | 0.017 | 0.043 | 2.6x |
| zero-length line | 0.015 | 0.652 | 43x |
| valid ls 4v | 0.034 | 0.046 | 1.3x |
| valid ls 10v | 0.048 | 0.082 | 1.7x |
| valid ls 50v | 0.104 | 0.095 | 0.92x |
| valid ls 100v | 0.173 | 0.155 | 0.89x |
| valid ls 500v | 0.705 | 0.588 | 0.83x |
| valid ls 1000v | 1.7 | 3.3 | 1.9x |
| collinear ls 4v | 0.03 | 0.573 | 19x |
| collinear ls 10v | 0.049 | 0.673 | 14x |
| collinear ls 50v | 0.108 | 0.802 | 7.4x |
| collinear ls 100v | 0.271 | 0.957 | 3.5x |
| collinear ls 500v | 0.815 | 2.4 | 3.0x |
| collinear ls 1000v | 1.8 | 11.8 | 6.6x |
| zigzag ls 10v | 0.056 | 1.4 | 24x |
| zigzag ls 50v | 0.114 | 5.3 | 46x |
| zigzag ls 100v | 0.198 | 10.7 | 54x |
| zigzag ls 500v | 0.611 | 65.7 | 107x |
| zigzag ls 1000v | 1.3 | 137 | 102x |
| spiral ls 10v | 0.125 | 1.1 | 8.8x |
| spiral ls 50v | 1.0 | 5.9 | 5.7x |
| spiral ls 100v | 2.1 | 18.1 | 8.6x |
| spiral ls 500v | 110 | 636 | 5.8x |
| spiral ls 1000v | 328 | 2041 | 6.2x |
| self-int ls 100v | 13.3 | 2.7 | 0.20x |
| self-int ls 500v | 11.9 | 4.1 | 0.34x |
| self-int ls 1000v | 23.0 | 7.2 | 0.31x |
| dense self ls 10v | 0.049 | 1.4 | 28x |
| dense self ls 50v | 0.095 | 6.6 | 70x |
| dense self ls 100v | 0.171 | 14.2 | 83x |
| dense self ls 500v | 1.1 | 169 | 152x |
| dense self ls 1000v | 1.4 | 391 | 289x |
| duped ls 100v | 0.258 | 0.712 | 2.8x |
| duped ls 500v | 0.702 | 1.5 | 2.2x |
| duped ls 1000v | 0.935 | 4.8 | 5.1x |
| mls 50x3v | 6.4 | 37.3 | 5.9x |
| mls 250x3v | 26.8 | 142 | 5.3x |
| mls 500x3v | 53.5 | 285 | 5.3x |
| mls 1000x3v | 103 | 573 | 5.6x |
| mls 10000x3v | 2249 | 13376 | 5.9x |
| self-int mls 50x4v | 38.2 | 82.6 | 2.2x |
| self-int mls 250x4v | 200 | 463 | 2.3x |
| self-int mls 500x4v | 371 | 1161 | 3.1x |
| star-burst 10sp | 4.1 | 15.4 | 3.7x |
| star-burst 50sp | 26.1 | 281 | 11x |
| star-burst 100sp | 5.5 | 1108 | 203x |
| star-burst 500sp | 22.3 | 36981 | 1661x |
| star-burst 1000sp | 44.0 | 184765 | 4195x |
| collinear ov 10seg | 1.3 | 12.5 | 9.3x |
| collinear ov 50seg | 6.9 | 57.4 | 8.3x |
| collinear ov 100seg | 14.1 | 110 | 7.8x |
| collinear ov 500seg | 63.6 | 576 | 9.1x |
| collinear ov 1000seg | 158 | 1183 | 7.5x |
| x-scale 10v | 4.4 | 23.3 | 5.3x |
| x-scale 50v | 55.0 | 395 | 7.2x |
| x-scale 100v | 283 | 1598 | 5.7x |
| x-scale 500v | 23519 | 57521 | 2.4x |
| x-scale 1000v | 1853 | 332587 | 180x |
| ringing 100v | 8.2 | 67.3 | 8.2x |
| ringing 500v | 31.0 | 322 | 10x |
| ringing 1000v | 72.2 | 982 | 14x |
| hilbert 256v | 69.8 | 213 | 3.0x |
| hilbert 1024v | 262 | 1378 | 5.3x |
| lissajous 200v | 30.8 | 152 | 4.9x |
| lissajous 500v | 57.9 | 415 | 7.2x |
| lissajous 1000v | 113 | 793 | 7.0x |
| lissajous 2000v | 287 | 1718 | 6.0x |
| lissajous 5000v | 900 | 6559 | 7.3x |
| lissajous 7:4 500v | 44.3 | 53.7 | 1.2x |
| spoke 10sp | 3.9 | 15.3 | 3.9x |
| spoke 50sp | 1.4 | 256 | 190x |
| spoke 100sp | 3.7 | 1102 | 300x |
| spoke 500sp | 24.9 | 39913 | 1605x |
| spoke 1000sp | 50.5 | 202606 | 4011x |
| star-comb 20sp | 0.411 | 3.6 | 8.7x |
| star-comb 100sp | 5.9 | 41.5 | 7.1x |
| star-comb 500sp | 79.4 | 863 | 11x |
| star-comb 1000sp | 721 | 3191 | 4.4x |
| hole hier 5h | 1.3 | 1.8 | 1.4x |
| hole hier 20h | 5.7 | 8.0 | 1.4x |
| hole hier 50h | 22.5 | 24.8 | 1.1x |
| hole hier 100h | 46.1 | 53.3 | 1.2x |
| overlap mp 5sh | 7.9 | 510 | 65x |
| overlap mp 20sh | 80.6 | 2719 | 34x |
| overlap mp 50sh | 482 | 7109 | 15x |
| overlap mp 100sh | 1605 | 14673 | 9.1x |
| dense grid 5x5=25 | 11.9 | 1770 | 149x |
| dense grid 10x10=100 | 53.3 | 12547 | 235x |
| dense grid 20x20=400 | 369 | 101100 | 274x |
| dense grid 30x30=900 | 1032 | 379336 | 368x |
| sliver 100v | 1.9 | 2.4 | 1.3x |
| sliver 500v | 8.7 | 22.9 | 2.6x |
| sliver 1000v | 16.6 | 41.0 | 2.5x |
| arrange valid 4v | 0.08 | 0.311 | 3.9x |
| arrange valid 10v | 0.119 | 0.507 | 4.2x |
| arrange valid 50v | 0.55 | 0.804 | 1.5x |
| arrange valid 100v | 0.711 | 0.809 | 1.1x |
| arrange valid 500v | 2.1 | 1.8 | 0.87x |
| arrange valid 1000v | 4.2 | 5.4 | 1.3x |
| arrange bowtie 4v | 1.0 | 23.8 | 23x |
| arrange bowtie 100v | 26.9 | 121 | 4.5x |
| arrange star 10sp | 3.7 | 16.0 | 4.4x |
| arrange star 50sp | 24.7 | 320 | 13x |
| arrange star 100sp | 3.7 | 1324 | 361x |
| arrange star 500sp | 21.9 | 41965 | 1913x |

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
   otherwise, with one exception below the snap grid: when `|coord| * 1e8`
   overflows the 2^53 key space, an empty repair returns the input with
   consecutive duplicate vertices collapsed, because the input's features
   are finer than the grid can resolve and the loss cannot be measured.
3. **W/12 pool-saturation floor:** the parallel batch fills all 12 workers
   with giants; nested intra-poly rayon finds no idle threads in-batch
   (Amdahl-bound; standalone the parallel check dropped from 96 ms to 53 ms).
4. **Giants (>4,096 ring edges) route to the boolean pipeline** - on a
   200k-edge giant single-pass noding costs 168 ms vs 36 ms for the
   boolean path (`SP_MAX_EDGES` in `src/core/mod.rs`).
5. **Mass-overlap repairs are the slowest synthetic class** (~1.5 ms for a
   100-shell overlap) but still beat GEOS makeValid by 10x to 61x on the overlap
   rows and 180x to 398x on the dense grid.
6. **Python bindings: `tests/test_python.py` covers the WKB and WKT surface**
   (41 tests); GeoJSON bindings were removed deliberately.
7. **`simd-portable` is nightly-only** (3 E0554 on stable, expected);
   hand-written AVX2 beyond the bbox scan measured slower than
   auto-vectorized scalar (point_in_ring 8.4x, is_ring_ccw 2.8x), and
   `-C target-cpu=native` regresses the full pass ~25%.
8. **`proj` requires native PROJ and is mutually exclusive with `io-gpkg`**
   (sqlite3 link conflict). `io-gpkg` is opt-in, so a `proj` build enables
   `proj` and leaves `io-gpkg` off.
9. **WKT reads a dimension modifier, it does not infer one from the ordinate
   count:** `POINT Z (1 2 3)` parses, `POINT (1 2 3)` stays an error.

The measured follow-ups are in [ROADMAP.md](ROADMAP.md).

## Integration with the geo ecosystem

geo-repair plugs into georust two ways: a `geo-traits` interop module that
validates or repairs any `GeometryTrait` source (geo, geoarrow, geozero,
`wkb`) in one call, and a `GeoRepairValidation` adapter that exposes our
validator through geo's own `Validation` trait. Details and examples:
[`docs/INTEROP.md`](docs/INTEROP.md).

## Embedding surface

Four pieces exist for a host application: a batch entry that builds with or
without the thread pool, an opt-in output policy, a delivery adapter for a
closed host model, and error classes.

- `batch::{fix_polygon_batch, fix_polygon_batch_owned, fix_polygon_batch_chunked}`
  run on rayon when `parallel` is enabled, and on a serial loop when it is not.
- `output::{OutputPolicy, apply_policy, make_valid_with_policy}` reduce a result
  to one homogeneous geometry for a host that has no collection type.
  `OutputReport` counts the parts kept and dropped.
- `host::{to_host, to_host_with, to_host_wkb, to_host_ewkt}` deliver geometry that
  a closed 2D host can store. `to_host_zm`, `to_host_wkb_zm` and
  `to_host_ewkt_zm` keep Z and M beside it. `HostReport::notices` gives one
  notice per change, with its count.
- `GeometryValidationError::code()` gives a stable key, `label()` a short name,
  and `class()` one of six `ValidationClass` values.
  `ValidationResult::class_counts()` totals them per class.

The default build carries no thread pool and no bundled SQLite. Enable
`parallel` and `io-gpkg`, or the `full` set, when the host wants them.

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
the WKB batch API. Mind that due to overhead it might suffer from performance issues.
Examples and full API reference: `docs/BINDINGS.md`.

## C API

The `ffi` feature exposes a panic-safe C API over WKB and WKT, single
geometries and parallel batches. Build with
`cargo build --release --features ffi,parallel`; the output is
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
| `parallel` | Rayon parallel processing (non-WASM); the `batch` entries fall back to a serial loop without it | no |
| `simd` | No-op on stable (kernels compiled unconditionally, LLVM auto-vectorized; hand-written AVX2 measured slower, kept only for the nightly `simd-portable` path) | no |
| `validate` | OGC validation predicates | yes |
| `rstar` | R-tree index used by noding (enabled transitively by `arrange`/`structure`/`validate`) | yes (transitive) |
| `mimalloc` | Use mimalloc global allocator | yes |
| `full` | The desktop set: default plus `parallel` and `io-gpkg` | no |
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
| `io-gpkg` | GeoPackage format backend (gated out on wasm32) | no |
| `io-all` | All opt-in backends except gpkg | no |
| `io-all-native` | All opt-in backends including gpkg | no |
| `hotpath` | Dev-only: `hotpath::measure` instrumentation on hot paths | no |
| `hotpath-alloc` | Dev-only: `hotpath` instrumentation plus allocation counters | no |
| `bench-geos` | GEOS comparison benchmarks (build from source, MSVC, no LTO) | no |
| `bench-geos-system` | GEOS comparison benchmarks (link against system GEOS, conda-forge MSVC) | no |
| `bench-gdal-system` | GDAL/OGR I/O comparison benchmark (link against system GDAL) | no |

## License

Apache-2.0

GEOS is an inspiration in the broad sense: it defines the repair behaviour this
crate targets, and the model that wrote the core may have seen GEOS in training
data. The core was written from `docs/REPAIR-CORE-SPEC.md`, not from the source.
No GEOS or rttopo code ships.

Portions of the validation topology and face extraction derive from the JTS
Topology Suite under the Eclipse Distribution License 1.0. See `NOTICE`.

`tests/geos_xml` is test data. 64 files were re-imported from JTS (EDL 1.0); the
rest are GEOS data under LGPL-2.1-or-later. The directory is excluded from the
published crate.

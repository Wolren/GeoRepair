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
  extraction, face walking, even-odd assembly.
- **Arrange** is a CDT-based fallback for complex topologies.

WKB and WKT reading and writing, plus a custom binary batch format, are
built in with no extra dependencies.

WKT keeps Z and M when you ask for it: `read_wkt_zm` returns the geometry, its
`Dims` and the extra ordinates in flat coordinate order (the same layout as
`read_ewkb`), and `write_wkt_zm` writes the modifier back. `read_wkt` reads the
same files and returns x/y, so a Z-enabled layer from OGR or QGIS loads without
error.

Everything is evaluated against a large test set ported from GEOS and CGAL, plus the crate's
own fuzz tests. The result should always come out valid per GEOS, but the strategy differences
can make the output slightly differ.

## Performance

### A test dataset (1,579,030 polygons)

Structure batch on a GIS dataset (813 features from a GeoPackage,
flattened to 1,579,030 polygon parts), measured 2026-09-18. i5-12400F
(6C/12T), release LTO, Rayon 12-thread batch; GEOS is conda-forge 3.14.1,
serial per-call, run concurrently via Rayon, geometries built via CoordSeq
direct construction (the one-time 1.5 s pre-build is excluded).

| Dataset | GeoRepair | GEOS | Ratio |
|---------|----------:|-----:|------:|
| Validation (1.58M) | **2.1 s** (1.3 µs/poly) | 3.0 s (1.9 µs/poly) | **1.4x** |
| Full pass (1.58M) | **2.1 s** (1.3 µs/poly) | 3.2 s (2.1 µs/poly) | **1.5x** |

### Synthetic benchmarks

Full table, parallel batch (µs); ratio = GEOS / GeoRepair, >1 means
we win. Methodology: `docs/BENCHMARKS.md`. Regenerate:
`python scripts/readme_bench_table.py --update <bench.json>`; CI gate:
`python scripts/readme_bench_table.py --check <bench.json>`. The overlap and
dense grid rows were re-measured 2026-11-05; the rest are the 2026-09-18 set.

| Benchmark | GeoRepair | GEOS | Ratio |
|-----------|----------:|-----:|------:|
| valid polygon 4v | 0.051 | 0.237 | 4.7x |
| valid polygon 10v | 0.137 | 0.319 | 2.3x |
| valid polygon 50v | 0.245 | 0.415 | 1.7x |
| valid polygon 100v | 0.408 | 0.578 | 1.4x |
| valid polygon 500v | 1.4 | 1.7 | 1.2x |
| valid polygon 1000v | 2.4 | 2.7 | 1.1x |
| valid polygon 5000v | 11.1 | 8.4 | 0.76x |
| valid polygon 10000v | 21.4 | 17.4 | 0.81x |
| invalid bowtie 4v | 0.661 | 19.9 | 30x |
| invalid bowtie 50v | 7.7 | 65.8 | 8.5x |
| invalid bowtie 100v | 25.1 | 118 | 4.7x |
| invalid bowtie 500v | 56.9 | 511 | 9.0x |
| invalid bowtie 1000v | 109 | 1001 | 9.2x |
| star poly 100v | 8.6 | 6.6 | 0.76x |
| star poly 500v | 172 | 91.4 | 0.53x |
| star poly 1000v | 241 | 292 | 1.2x |
| spaghetti 500v | 1589 | 2633 | 1.7x |
| spaghetti 2000v | 11391 | 16251 | 1.4x |
| self-touch 100v | 0.493 | 0.759 | 1.5x |
| self-touch 500v | 1.5 | 1.6 | 1.0x |
| self-touch 1000v | 2.8 | 3.2 | 1.1x |
| collapsed 100v | 21.3 | 137 | 6.4x |
| collapsed 500v | 113 | 554 | 4.9x |
| collapsed 1000v | 189 | 1154 | 6.1x |
| near-collinear 100v | 73.1 | 191 | 2.6x |
| near-collinear 500v | 874 | 1184 | 1.4x |
| near-collinear 1000v | 2880 | 2857 | 0.99x |
| large coord 1e12 100v | 0.438 | 0.472 | 1.1x |
| large coord 1e12 500v | 1.2 | 1.1 | 0.91x |
| large coord 1e12 1000v | 2.6 | 2.6 | 1.0x |
| valid line | 0.009 | 0.039 | 4.1x |
| zero-length line | 0.007 | 0.247 | 36x |
| valid ls 4v | 0.022 | 0.039 | 1.7x |
| valid ls 10v | 0.037 | 0.044 | 1.2x |
| valid ls 50v | 0.078 | 0.095 | 1.2x |
| valid ls 100v | 0.146 | 0.126 | 0.86x |
| valid ls 500v | 0.406 | 0.495 | 1.2x |
| valid ls 1000v | 1.4 | 2.6 | 1.8x |
| collinear ls 4v | 0.022 | 0.563 | 26x |
| collinear ls 10v | 0.037 | 0.58 | 16x |
| collinear ls 50v | 0.067 | 0.72 | 11x |
| collinear ls 100v | 0.121 | 0.943 | 7.8x |
| collinear ls 500v | 0.477 | 2.0 | 4.1x |
| collinear ls 1000v | 1.3 | 8.6 | 6.5x |
| zigzag ls 10v | 0.042 | 1.3 | 31x |
| zigzag ls 50v | 0.074 | 4.7 | 63x |
| zigzag ls 100v | 0.126 | 10.0 | 79x |
| zigzag ls 500v | 0.493 | 56.0 | 113x |
| zigzag ls 1000v | 1.1 | 118 | 108x |
| spiral ls 10v | 0.096 | 0.835 | 8.7x |
| spiral ls 50v | 0.704 | 4.3 | 6.1x |
| spiral ls 100v | 1.6 | 16.1 | 10x |
| spiral ls 500v | 73.7 | 551 | 7.5x |
| spiral ls 1000v | 270 | 1861 | 6.9x |
| self-int ls 100v | 1.2 | 2.6 | 2.2x |
| self-int ls 500v | 5.1 | 3.8 | 0.75x |
| self-int ls 1000v | 9.1 | 6.5 | 0.72x |
| dense self ls 10v | 0.043 | 1.2 | 28x |
| dense self ls 50v | 0.094 | 5.3 | 57x |
| dense self ls 100v | 0.177 | 11.7 | 66x |
| dense self ls 500v | 0.671 | 122 | 181x |
| dense self ls 1000v | 1.1 | 349 | 305x |
| duped ls 100v | 0.268 | 0.759 | 2.8x |
| duped ls 500v | 0.492 | 1.4 | 2.8x |
| duped ls 1000v | 0.738 | 3.1 | 4.2x |
| mls 50x3v | 2.7 | 30.2 | 11x |
| mls 250x3v | 15.2 | 132 | 8.7x |
| mls 500x3v | 36.5 | 252 | 6.9x |
| self-int mls 50x4v | 17.6 | 77.5 | 4.4x |
| self-int mls 250x4v | 130 | 466 | 3.6x |
| self-int mls 500x4v | 402 | 1034 | 2.6x |
| star-burst 10sp | 3.0 | 14.6 | 4.8x |
| star-burst 50sp | 0.707 | 249 | 353x |
| star-burst 100sp | 1.3 | 1085 | 814x |
| star-burst 500sp | 7.6 | 36693 | 4838x |
| star-burst 1000sp | 14.2 | 188647 | 13304x |
| collinear ov 10seg | 1.1 | 13.1 | 12x |
| collinear ov 50seg | 7.4 | 60.5 | 8.2x |
| collinear ov 100seg | 13.5 | 117 | 8.7x |
| collinear ov 500seg | 62.2 | 628 | 10x |
| collinear ov 1000seg | 182 | 1426 | 7.8x |
| x-scale 10v | 2.4 | 21.1 | 8.8x |
| x-scale 50v | 63.4 | 436 | 6.9x |
| x-scale 100v | 306 | 1689 | 5.5x |
| x-scale 500v | 24135 | 59625 | 2.5x |
| x-scale 1000v | 1935 | 371599 | 192x |
| ringing 100v | 7.5 | 66.9 | 8.9x |
| ringing 500v | 27.9 | 364 | 13x |
| ringing 1000v | 65.6 | 991 | 15x |
| hilbert 256v | 35.5 | 171 | 4.8x |
| hilbert 1024v | 219 | 1344 | 6.1x |
| lissajous 200v | 37.0 | 174 | 4.7x |
| lissajous 500v | 58.6 | 419 | 7.2x |
| lissajous 1000v | 99.9 | 854 | 8.5x |
| lissajous 2000v | 230 | 1726 | 7.5x |
| lissajous 5000v | 671 | 6131 | 9.1x |
| lissajous 7:4 500v | 46.2 | 49.8 | 1.1x |
| spoke 10sp | 3.4 | 14.8 | 4.3x |
| spoke 50sp | 0.816 | 250 | 307x |
| spoke 100sp | 1.5 | 1098 | 739x |
| spoke 500sp | 10.7 | 36970 | 3442x |
| spoke 1000sp | 20.4 | 201201 | 9844x |
| star-comb 20sp | 0.386 | 2.7 | 7.0x |
| star-comb 100sp | 5.6 | 33.0 | 5.9x |
| star-comb 500sp | 78.4 | 679 | 8.7x |
| star-comb 1000sp | 668 | 2607 | 3.9x |
| hole hier 5h | 1.5 | 1.8 | 1.2x |
| hole hier 20h | 5.3 | 7.5 | 1.4x |
| hole hier 50h | 19.4 | 26.1 | 1.3x |
| hole hier 100h | 44.5 | 50.9 | 1.1x |
| overlap mp 5sh | 7.5 | 452 | 61x |
| overlap mp 20sh | 70.4 | 2727 | 39x |
| overlap mp 50sh | 451 | 6972 | 15x |
| overlap mp 100sh | 1484 | 15482 | 10x |
| dense grid 5x5=25 | 9.7 | 1751 | 180x |
| dense grid 10x10=100 | 45.8 | 13017 | 284x |
| dense grid 20x20=400 | 341 | 97618 | 286x |
| dense grid 30x30=900 | 996 | 396084 | 398x |
| sliver 100v | 1.3 | 2.5 | 1.9x |
| sliver 500v | 7.3 | 17.6 | 2.4x |
| sliver 1000v | 14.0 | 27.4 | 2.0x |
| arrange valid 4v | 0.059 | 0.275 | 4.6x |
| arrange valid 10v | 0.102 | 0.353 | 3.4x |
| arrange valid 50v | 0.426 | 0.484 | 1.1x |
| arrange valid 100v | 0.498 | 0.719 | 1.4x |
| arrange valid 500v | 1.9 | 2.0 | 1.1x |
| arrange valid 1000v | 3.5 | 3.6 | 1.0x |
| arrange bowtie 4v | 0.981 | 16.3 | 17x |
| arrange bowtie 100v | 23.1 | 116 | 5.0x |
| arrange star 10sp | 2.7 | 14.7 | 5.5x |
| arrange star 50sp | 1.0 | 261 | 250x |
| arrange star 100sp | 2.4 | 1189 | 491x |
| arrange star 500sp | 7.4 | 42434 | 5747x |

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
   count:** `POINT Z (1 2 3)` parses, `POINT (1 2 3)` stays an error, where GEOS
   infers Z from the third ordinate.

## Integration with the geo ecosystem

geo-repair plugs into georust two ways: a `geo-traits` interop module that
validates or repairs any `GeometryTrait` source (geo, geoarrow, geozero,
`wkb`) in one call, and a `GeoRepairValidation` adapter that exposes our
validator through geo's own `Validation` trait. Details and examples:
[`docs/INTEROP.md`](docs/INTEROP.md).

## Embedding surface

Three pieces exist for a host application: a batch entry that builds with or
without the thread pool, an opt-in output policy, and error classes.

- `batch::{fix_polygon_batch, fix_polygon_batch_owned, fix_polygon_batch_chunked}`
  run on rayon when `parallel` is enabled, and on a serial loop when it is not.
- `output::{OutputPolicy, apply_policy, make_valid_with_policy}` reduce a result
  to one homogeneous geometry for a host that has no collection type.
  `OutputReport` counts the parts kept and dropped.
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
the WKB batch API. Mind  that due to overhead it might suffer from performance issues.
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

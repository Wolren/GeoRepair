# GeoRepair

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.85+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)
[![Status](https://img.shields.io/badge/status-experimental-orange.svg)]()

> **This crate is experimental.**  The API is actively evolving — expect
> breaking changes between 0.x releases.  Core algorithms, I/O backends,
> and feature flags are all subject to change as we improve correctness
> and performance.

OGC geometry repair and validation for Rust. Built-in I/O for WKB, WKT,
and a custom binary batch format — no extra dependencies required.
Detects and fixes invalid
GIS geometries (self-intersections, unclosed rings, degenerate shapes, NaN
coordinates, and more) using algorithms selected by geometry type.

The **Structure** strategy (default) mirrors GEOS's ST_MakeValid
algorithm: planar graph extraction, face walking, and winding-number
assembly.  The **Arrange** strategy uses CDT-based repair as a robust
fallback for complex topologies.  Passes 2490/2490 GEOS XML validation
tests, with parallel batch performance **0.30× GEOS** (3.3× faster) on
1.58M data set polygons.

See the [full documentation](https://docs.rs/geo-repair) for quick-start
examples, validation rules, CRS support, I/O backends, Python bindings,
C FFI, and known limitations.

## What it does

Real-world GIS data often contains geometry defects.  GeoRepair detects
these via OGC-style validation and repairs them:

| Geometry | Repair approach |
|----------|----------------|
| `Polygon` / `MultiPolygon` | Two strategies (Structure fast path / Arrange CDT fallback) |
| `LineString` / `MultiLineString` | NaN filtering, duplicate removal, self-intersection noding |
| `Line` | Zero-length and NaN detection |
| `Point` / `MultiPoint` | NaN/Inf filtering, deduplication |
| `Rect` / `Triangle` | Basic degeneracy checks |
| `GeometryCollection` | Recursive repair of children |

## Performance

### Real-world dataset (1,578,988 polygons)

Structure parallel batch on a production GIS dataset.  GEOS linked via
conda (LLVM build, parallel, full LTO).  i5-12400F (6C/12T), mimalloc.

| Dataset | geo-repair | GEOS (system LLVM) | Ratio |
|---------|------------|-------------------|-------|
| Invalid subset (1855 polys) | **2.54 s** / 1.37 ms each | **3.28 s** / 1.77 ms each | **0.77×** |
| Full dataset (1.58M polys) | **3.51 s** / 2.2 µs each | **3.82 s** / 2.4 µs each | **0.92×** |

Validation comparison on full dataset — our validator is **5.2× faster**:

| Validator | total | per-poly |
|-----------|-------|----------|
| Geo-repair | 0.79 s | 0.50 µs |
| GEOS isValid | 4.16 s | 2.64 µs |

### Synthetic benchmarks (parallel, grid+R-tree hybrid)

Structure strategy, parallel batch, i5-12400F (6C/12T).  GEOS via WKT conversion
(system LLVM build, parallel, full LTO — maxed out).

| Benchmark | geo-repair | GEOS (LLVM) | Ratio |
|-----------|-----------:|------------:|------:|
| Valid polygon 4v | 0.09 us | 4.37 us | 48× |
| Valid polygon 10v | 0.18 us | 7.61 us | 44× |
| Valid polygon 50v | 0.44 us | 26.7 us | 61× |
| Valid polygon 100v | 0.47 us | 50.5 us | 107× |
| Valid polygon 500v | 2.12 us | 259 us | 122× |
| Valid polygon 1000v | 2.90 us | 502 us | 173× |
| Valid polygon 5000v | 17.0 us | 2239 us | 132× |
| Valid polygon 10000v | 34.3 us | 4452 us | 130× |
| Invalid bowtie 4v | 0.38 us | 94.1 us | 250× |
| Invalid star 100v | 4.82 us | 92.8 us | 19× |
| Collinear ls 4v | 0.03 us | 2.15 us | 65× |
| Collinear ls 10v | 0.10 us | 3.20 us | 34× |
| Collinear ls 50v | 1.63 us | 9.20 us | 6× |
| Collinear ls 100v | 1.53 us | 16.3 us | 11× |
| Collinear ls 500v | 9.76 us | 74.1 us | 8× |
| Hilbert curve 256v | 86.0 us | 43.2 us | GEOS 2× |
| Hilbert curve 1024v | 649 us | 166 us | GEOS 4× |
| Lissajous 200v | 60.4 us | 89.7 us | 1.5× |
| Lissajous 500v | 140 us | 222 us | 1.6× |
| Lissajous 1000v | 282 us | 455 us | 1.6× |
| Star-burst 10sp | 0.29 us | 7.08 us | 24× |
| Star-burst 50sp | 10.3 us | 34.7 us | 3.4× |
| Star-burst 100sp | 36.7 us | 59.0 us | 1.6× |
| Star-burst 500sp | 1028 us | 294 us | GEOS 3.5× |
| Spoke wheel 10sp | 4.92 us | 7.17 us | 1.5× |
| Spoke wheel 50sp | 40.7 us | 31.1 us | GEOS 1.3× |
| Spoke wheel 100sp | 159 us | 62.1 us | GEOS 2.6× |
| Spoke wheel 500sp | 9415 us | 295 us | GEOS 32× |
| Collinear overlap 10seg | 4.39 us | 6.14 us | 1.4× |
| Collinear overlap 50seg | 22.6 us | 24.7 us | 1.1× |
| Collinear overlap 100seg | 48.6 us | 46.9 us | GEOS 1.0× |
| Collinear overlap 500seg | 282 us | 228 us | GEOS 1.2× |
| Hole hierarchy 5h | 1.00 us | 24.5 us | 25× |
| Hole hierarchy 20h | 3.66 us | 123 us | 34× |
| Hole hierarchy 50h | 9.32 us | 299 us | 32× |
| Overlapping MP 5sh | 1.70 us | 2561 us | 1507× |
| Overlapping MP 20sh | 5.96 us | 15044 us | 2524× |
| Overlapping MP 50sh | 15.8 us | 38399 us | 2427× |
| Sliver polygon 100v | 1.56 us | 79.2 us | 51× |
| Sliver polygon 500v | 9.19 us | 406 us | 44× |

**Arrange pipeline (CDT fallback):**

| Benchmark | geo-repair | GEOS (LLVM) | Ratio |
|-----------|-----------:|------------:|------:|
| Valid polygon 4v | 0.12 us | 3.91 us | 33× |
| Valid polygon 50v | 1.48 us | 26.4 us | 18× |
| Invalid bowtie 4v | 0.67 us | 90.7 us | 136× |
| Star-burst 10sp | 0.32 us | 7.33 us | 23× |
| Star-burst 50sp | 11.5 us | 32.1 us | 2.8× |

### Run benchmarks

```shell
# Real-world dataset benchmark (system GEOS — conda LLVM, fastest)
cargo bench --features bench-geos-system,arrange,structure,parallel,simd,io-shp --bench real_world

# Real-world dataset benchmark (static GEOS — MSVC, no LTO)
cargo bench --features bench-geos,arrange,structure,parallel,simd,io-shp --bench real_world

# Quick synthetic benchmarks (no GEOS)
cargo bench --bench bench

# Synthetic benchmarks with GEOS comparison
cargo bench --features bench-geos-system --bench bench

# Criterion microbenchmarks
cargo bench --features bench-criterion --bench criterion
```

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `arrange` | CDT-based polygon repair (requires `spade`) | yes |
| `structure` | Structure-based fast path repair | yes |
| `parallel` | Rayon parallel processing (non-WASM) | yes |
| `simd` | AVX2-accelerated orientation tests (x86_64) | yes |
| `validate` | OGC validation predicates | yes |
| `mimalloc` | Use mimalloc global allocator | yes |
| `std` | Standard library + file I/O. Disable for no_std builds. | yes |
| `simd-portable` | Portable SIMD via `core::simd` (nightly only) | no |
| `memmap` | Memory-mapped binary file loading | no |
| `wasm` | WASM browser fetch (synchronous XHR) | no |
| `proj` | CRS transformation (placeholder) | no |
| `serde` | Geometry serde support (`geo/serde`) | no |
| `ffi` | C-compatible FFI bindings | no |
| `python` | Python bindings via PyO3 | no |
| `io-shp` | Shapefile format backend | no |
| `io-wkt` | No-op (WKT is built-in, kept for CI compatibility) | no |
| `io-csv` | CSV format backend | no |
| `io-gml` | GML/XML format backend | no |
| `io-gpkg` | GeoPackage format backend (not WASM) | no |
| `io-all` | All opt-in backends except gpkg | no |
| `io-all-native` | All opt-in backends including gpkg | no |
| `bench-geos` | GEOS comparison benchmarks (static — MSVC, no LTO) | no |
| `bench-geos-system` | GEOS comparison benchmarks (system — conda LLVM) | no |
| `bench-criterion` | Criterion benchmark harness | no |

## License

Apache-2.0

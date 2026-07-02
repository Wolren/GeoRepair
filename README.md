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
tests, with parallel batch performance **on par with GEOS** on
1.58M data set polygons (full dataset 0.92×, validation 3.4×).

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
conda-forge (MSVC, serial internally, no LTO).  i5-12400F (6C/12T),
mimalloc.  Both benchmarks run in parallel via Rayon batch over all
polygons (GEOS per-poly serial, but many polys run concurrently).

| Dataset | geo-repair | GEOS | Ratio |
|---------|------------|------|-------|
| Invalid subset (1855 polys) | — | **1.88 s** / 1.02 ms each | — |
| Full dataset (1.58M polys) | **3.34 s** / 2.1 µs each | **3.61 s** / 2.3 µs each | **0.92×** |

Validation comparison on full dataset — our validator is **3.4× faster**:

| Validator | total | per-poly |
|-----------|-------|----------|
| Geo-repair | 1.13 s | 0.71 µs |
| GEOS isValid | 3.78 s | 2.40 µs |

### Synthetic benchmarks

Structure strategy, i5-12400F (6C/12T).  GEOS linked via conda-forge
(MSVC, serial, no LTO).  GEOS call includes WKT round-trip (encode to
WKT, decode by GEOS).  GeoRepair serial column is apples-to-apples
(single-threaded); parallel column shows the Rayon batch speedup.

| Benchmark | GeoRepair (ser) | GeoRepair (par) | GEOS (par batch) | Ratio (ser) |
|-----------|----------------:|----------------:|-----------------:|------------:|
| Valid polygon 4v | 0.21 µs | 0.10 µs | 3.79 µs | 18× |
| Valid polygon 50v | 0.43 µs | 0.32 µs | 5.13 µs | 12× |
| Valid polygon 500v | 3.06 µs | 1.77 µs | 34.1 µs | 11× |
| Valid polygon 10000v | 49.5 µs | 37.6 µs | 647 µs | 13× |
| Invalid bowtie 4v | 2.09 µs | 0.33 µs | 17.8 µs | 8.5× |
| Invalid star 100v | 25.6 µs | 4.46 µs | 22.5 µs | 0.9× |
| Self-touching poly | 4.13 µs | 1.10 µs | 20.8 µs | 5.0× |
| Collapsed poly | 0.76 µs | 0.19 µs | 27.9 µs | 37× |
| Near-collinear poly | 1.40 µs | 0.44 µs | 42.5 µs | 30× |
| Hilbert curve 256v | 0.58 µs | 0.56 µs | 14.3 µs | 25× |
| Hilbert curve 1024v | 2.38 µs | 1.93 µs | 34.7 µs | 15× |
| Lissajous 200v | 0.39 µs | 0.46 µs | 20.5 µs | 52× |
| Lissajous 1000v | 3.50 µs | 4.28 µs | 98.7 µs | 28× |
| Star-burst 10sp | 0.27 µs | 0.07 µs | 7.26 µs | 27× |
| Star-burst 50sp | 0.95 µs | 0.21 µs | 12.9 µs | 14× |
| Star-burst 100sp | 1.97 µs | 0.32 µs | 24.7 µs | 13× |
| Star-burst 500sp | 8.99 µs | 1.26 µs | 122 µs | 14× |
| Spoke wheel 10sp | 0.22 µs | 0.06 µs | 5.91 µs | 27× |
| Spoke wheel 50sp | 0.73 µs | 0.15 µs | 8.06 µs | 11× |
| Spoke wheel 100sp | 2.10 µs | 0.43 µs | 14.0 µs | 6.7× |
| Spoke wheel 500sp | 8.71 µs | 1.32 µs | 71.6 µs | 8.2× |
| Star-comb 20sp | 0.23 µs | 0.10 µs | 6.73 µs | 29× |
| Star-comb 100sp | 0.81 µs | 0.20 µs | 12.1 µs | 15× |
| Star-comb 500sp | 4.02 µs | 0.83 µs | 48.3 µs | 12× |
| Collinear overlap 10seg | 0.29 µs | 0.08 µs | 5.11 µs | 18× |
| Collinear overlap 50seg | 1.19 µs | 0.24 µs | 5.21 µs | 4.4× |
| Collinear overlap 100seg | 2.50 µs | 0.49 µs | 8.15 µs | 3.3× |
| Collinear overlap 500seg | 10.8 µs | 1.95 µs | 41.3 µs | 3.8× |
| Hole hierarchy 5h | 1.85 µs | 1.17 µs | 8.24 µs | 4.5× |
| Hole hierarchy 20h | 5.56 µs | 3.65 µs | 14.2 µs | 2.6× |
| Hole hierarchy 50h | 17.5 µs | 12.8 µs | 59.7 µs | 3.4× |
| Overlapping MP 5sh | 3.98 µs | 1.41 µs | 421 µs | 106× |
| Overlapping MP 20sh | 19.3 µs | 6.72 µs | 2581 µs | 134× |
| Overlapping MP 50sh | 44.8 µs | 15.2 µs | 6649 µs | 148× |
| Dense grid 5×5=25 | 13.8 µs | 5.19 µs | 1631 µs | 118× |
| Dense grid 10×10=100 | 63.8 µs | 28.0 µs | 14005 µs | 220× |
| Dense grid 20×20=400 | 283 µs | 136 µs | 100012 µs | 353× |
| Sliver polygon 100v | 3.38 µs | 2.02 µs | 18.2 µs | 5.4× |
| Sliver polygon 500v | 16.6 µs | 7.60 µs | 72.4 µs | 4.4× |

**Arrange pipeline (CDT fallback):**

| Benchmark | GeoRepair (par) | GEOS (par batch) | Ratio |
|-----------|----------------:|-----------------:|------:|
| Valid polygon 4v | 0.09 µs | 3.93 µs | 42× |
| Valid polygon 50v | 1.65 µs | 5.62 µs | 3.4× |
| Invalid bowtie 4v | 0.65 µs | 18.1 µs | 28× |
| Star-burst 10sp | 0.09 µs | 7.25 µs | 79× |
| Star-burst 50sp | 0.51 µs | 12.8 µs | 25× |

**Notes on GEOS comparison:**
- conda-forge `libgeos` on Windows is compiled with MSVC, runs single-threaded internally, and does not use LTO. The "par batch" columns run many GEOS calls concurrently via Rayon (throughput parallelism), since GEOS per-call is serial.
- Synthetic GEOS numbers include WKT serialization/deserialization overhead. The real-world benchmark uses GEOS CoordSeq direct construction (no WKT) for fair comparison.
- GeoRepair parallel speedup is typically 1-3× on 12 cores for synthetic shapes (sub-5µs per call — Rayon overhead dominates). For real-world batches of 1.58M polygons, the throughput advantage is larger due to better amortization.

### Run benchmarks

```shell
# Real-world dataset benchmark (system GEOS — conda-forge)
cargo bench --features bench-geos-system,arrange,structure,parallel,simd,io-shp --bench real_world

# Real-world dataset benchmark (static GEOS — built from source)
cargo bench --features bench-geos,arrange,structure,parallel,simd,io-shp --bench real_world

# Synthetic benchmarks with serial + parallel columns (no GEOS)
cargo bench --features arrange,structure,parallel,simd --bench bench

# Synthetic benchmarks with GEOS comparison
cargo bench --features bench-geos-system,arrange,structure,parallel,simd --bench bench

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
| `bench-geos` | GEOS comparison benchmarks (build from source — MSVC, no LTO) | no |
| `bench-geos-system` | GEOS comparison benchmarks (link against system GEOS — conda-forge MSVC) | no |
| `bench-criterion` | Criterion benchmark harness | no |

## License

Apache-2.0

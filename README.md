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

Structure parallel batch on a production GIS dataset.  GEOS geometries
built from CoordSeq (no WKT overhead).  i5-12400F (6C/12T), mimalloc.

| Dataset | geo-repair | GEOS (parallel) | Ratio |
|---------|------------|-----------------|-------|
| Invalid subset (1855 polys) | **2.21 s** / 1.19 ms each | **6.02 s** / 3.24 ms each | **0.37×** |
| Full dataset (1.58M polys) | **3.10 s** / 2.0 µs each | **10.18 s** / 6.4 µs each | **0.30×** |

GEOS agreement: **99.88%** (1855 disagreements where our validator is
stricter — GEOS does not detect these as invalid).

### Synthetic benchmarks (parallel, grid+R-tree hybrid)

Structure strategy, parallel batch, i5-12400F (6C/12T).  GEOS via WKT conversion.

| Benchmark | geo-repair | GEOS (parallel) | Ratio |
|-----------|------------|-----------------|-------|
| Valid polygon 4v | 0.12 us | 17.8 us | 144x |
| Valid polygon 10v | 0.71 us | 27.6 us | 39x |
| Valid polygon 50v | 0.39 us | 92.3 us | 240x |
| Valid polygon 100v | 0.80 us | 192 us | 242x |
| Valid polygon 500v | 5.17 us | 907 us | 175x |
| Valid polygon 1000v | 4.99 us | 1854 us | 372x |
| Valid polygon 5000v | 17.2 us | 7707 us | 447x |
| Valid polygon 10000v | 52.6 us | 16087 us | 306x |
| Invalid bowtie 4v | 0.56 us | 503 us | 893x |
| Invalid star 100v | 5.61 us | 409 us | 73x |
| Collinear ls 4v | 0.03 us | 10.0 us | 332x |
| Collinear ls 10v | 0.13 us | 17.0 us | 128x |
| Collinear ls 50v | 3.14 us | 53.9 us | 17x |
| Collinear ls 100v | 2.00 us | 114 us | 57x |
| Collinear ls 500v | 33.7 us | 541 us | 16x |
| Hilbert curve 256v | 145 us | 314 us | 2.2x |
| Hilbert curve 1024v | 1214 us | 1054 us | 0.87x (tie) |
| Lissajous 200v | 75.7 us | 359 us | 4.7x |
| Lissajous 500v | 311 us | 825 us | 2.7x |
| Lissajous 1000v | 624 us | 3419 us | 5.5x |
| Star-burst 10sp | 0.37 us | 30.9 us | 85x |
| Star-burst 50sp | 16.8 us | 154 us | 9.1x |
| Star-burst 100sp | 115 us | 329 us | 2.9x |
| Star-burst 500sp | 4261 us | 1383 us | GEOS 3.1x |
| Spoke wheel 10sp | 8.22 us | 33.2 us | 4.0x |
| Spoke wheel 50sp | 43.4 us | 109 us | 2.5x |
| Spoke wheel 100sp | 291 us | 220 us | GEOS 1.3x |
| Spoke wheel 500sp | 14550 us | 990 us | GEOS 14.7x |
| Collinear overlap 10seg | 13.8 us | 40.1 us | 2.9x |
| Collinear overlap 50seg | 51.7 us | 184 us | 3.6x |
| Collinear overlap 100seg | 133 us | 375 us | 2.8x |
| Collinear overlap 500seg | 882 us | 1507 us | 1.7x |
| Hole hierarchy 5h | 1.10 us | 79.6 us | 72x |
| Hole hierarchy 20h | 3.95 us | 323 us | 82x |
| Hole hierarchy 50h | 10.1 us | 851 us | 84x |
| Overlapping MP 5sh | 1.60 us | 7307 us | 4556x |
| Overlapping MP 20sh | 6.53 us | 47560 us | 7281x |
| Overlapping MP 50sh | 15.0 us | 123011 us | 8181x |
| Sliver polygon 100v | 1.58 us | 255 us | 162x |
| Sliver polygon 500v | 8.58 us | 1280 us | 149x |

**Arrange pipeline (CDT fallback):**

| Benchmark | geo-repair | GEOS (parallel) | Ratio |
|-----------|------------|-----------------|-------|
| Valid polygon 4v | 0.11 us | 12.2 us | 107x |
| Valid polygon 50v | 1.34 us | 72.3 us | 54x |
| Invalid bowtie 4v | 0.82 us | 339 us | 416x |
| Star-burst 10sp | 0.30 us | 23.8 us | 79x |
| Star-burst 50sp | 12.6 us | 104 us | 8.3x |

### Run benchmarks

```shell
# Real-world dataset benchmark (requires .bin file)
cargo bench --features bench-geos,arrange,structure,parallel,simd,io-shp --bench real_world

# Quick synthetic benchmarks
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
| `bench-geos` | GEOS comparison benchmarks | no |
| `bench-criterion` | Criterion benchmark harness | no |

## License

Apache-2.0

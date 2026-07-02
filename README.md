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

| Dataset | GeoRepair | Per-poly | GEOS | Per-poly | vs GEOS |
|---------|-----------|----------|------|----------|---------|
| Invalid subset (1855 polys) | **1.96 s** | 1.06 ms | **1.88 s** | 1.02 ms | *1.04×* |
| Full dataset (1.58M polys) | **3.34 s** | 2.1 µs | **3.61 s** | 2.3 µs | *0.92×* |

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

**Bold** ratio = ≥ 100× (massive).  *Italic* ratio = < 10× (modest).
**Bold** Ser/Par = ≥ 5× (good parallel scaling).

| Benchmark | Ser (µs) | Par (µs) | GEOS (µs) | Ratio (ser) | Ratio (par) | Ser/Par |
|-----------|---------:|---------:|----------:|------------:|------------:|--------:|
| Valid polygon 4v | 0.21 | 0.10 | 3.79 | 18× | *38×* | 2.1× |
| Valid polygon 50v | 0.43 | 0.32 | 5.13 | 12× | *16×* | 1.3× |
| Valid polygon 500v | 3.06 | 1.77 | 34.1 | 11× | *19×* | 1.7× |
| Valid polygon 10000v | 49.5 | 37.6 | 647 | 13× | *17×* | 1.3× |
| Invalid bowtie 4v | 2.09 | 0.33 | 17.8 | *8.5×* | *54×* | **6.3×** |
| Invalid star 100v | 25.6 | 4.46 | 22.5 | *0.9×* | *5.0×* | **5.7×** |
| Self-touching poly | 4.13 | 1.10 | 20.8 | *5.0×* | *19×* | 3.8× |
| Collapsed poly | 0.76 | 0.19 | 27.9 | *37×* | **147×** | 4.0× |
| Near-collinear poly | 1.40 | 0.44 | 42.5 | *30×* | *97×* | 3.2× |
| Hilbert curve 256v | 0.58 | 0.56 | 14.3 | *25×* | *26×* | 1.0× |
| Hilbert curve 1024v | 2.38 | 1.93 | 34.7 | *15×* | *18×* | 1.2× |
| Lissajous 200v | 0.39 | 0.46 | 20.5 | *53×* | *45×* | 0.8× |
| Lissajous 1000v | 3.50 | 4.28 | 98.7 | *28×* | *23×* | 0.8× |
| Star-burst 10sp | 0.27 | 0.07 | 7.26 | *27×* | **104×** | 3.9× |
| Star-burst 50sp | 0.95 | 0.21 | 12.9 | *14×* | *61×* | 4.5× |
| Star-burst 100sp | 1.97 | 0.32 | 24.7 | *13×* | *77×* | **6.2×** |
| Star-burst 500sp | 8.99 | 1.26 | 122 | *14×* | *97×* | **7.1×** |
| Spoke wheel 10sp | 0.22 | 0.06 | 5.91 | *27×* | *99×* | 3.7× |
| Spoke wheel 50sp | 0.73 | 0.15 | 8.06 | *11×* | *54×* | 4.9× |
| Spoke wheel 100sp | 2.10 | 0.43 | 14.0 | *6.7×* | *33×* | 4.9× |
| Spoke wheel 500sp | 8.71 | 1.32 | 71.6 | *8.2×* | *54×* | **6.6×** |
| Star-comb 20sp | 0.23 | 0.10 | 6.73 | *29×* | *67×* | 2.3× |
| Star-comb 100sp | 0.81 | 0.20 | 12.1 | *15×* | *61×* | 4.0× |
| Star-comb 500sp | 4.02 | 0.83 | 48.3 | *12×* | *58×* | 4.8× |
| Collinear overlap 10seg | 0.29 | 0.08 | 5.11 | *18×* | *64×* | 3.6× |
| Collinear overlap 50seg | 1.19 | 0.24 | 5.21 | *4.4×* | *22×* | **5.0×** |
| Collinear overlap 100seg | 2.50 | 0.49 | 8.15 | *3.3×* | *17×* | **5.1×** |
| Collinear overlap 500seg | 10.8 | 1.95 | 41.3 | *3.8×* | *21×* | **5.5×** |
| Hole hierarchy 5h | 1.85 | 1.17 | 8.24 | *4.5×* | *7.0×* | 1.6× |
| Hole hierarchy 20h | 5.56 | 3.65 | 14.2 | *2.6×* | *3.9×* | 1.5× |
| Hole hierarchy 50h | 17.5 | 12.8 | 59.7 | *3.4×* | *4.7×* | 1.4× |
| Overlapping MP 5sh | 3.98 | 1.41 | 421 | **106×** | **299×** | 2.8× |
| Overlapping MP 20sh | 19.3 | 6.72 | 2581 | **134×** | **384×** | 2.9× |
| Overlapping MP 50sh | 44.8 | 15.2 | 6649 | **148×** | **437×** | 2.9× |
| Dense grid 5×5=25 | 13.8 | 5.19 | 1631 | **118×** | **314×** | 2.7× |
| Dense grid 10×10=100 | 63.8 | 28.0 | 14005 | **220×** | **500×** | 2.3× |
| Dense grid 20×20=400 | 283 | 136 | 100012 | **353×** | **735×** | 2.1× |
| Sliver polygon 100v | 3.38 | 2.02 | 18.2 | *5.4×* | *9.0×* | 1.7× |
| Sliver polygon 500v | 16.6 | 7.60 | 72.4 | *4.4×* | *9.5×* | 2.2× |

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

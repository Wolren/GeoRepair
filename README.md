# geo-repair

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.95+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)

**Fix invalid GIS geometries** — detects and repairs broken polygons, lines, and points.

> **⚠️ EXPERIMENTAL — NOT PRODUCTION-READY**
>
> This crate is **pre-1.0, actively developed, and potentially unstable**. It aims to eventually
> produce OGC-compliant output for all valid inputs, but it may fail on edge cases, produce
> unexpected results, or panic. The API will change. Use at your own risk.
>
> **When it works, it's fast. When it doesn't, it fails loudly.** See [known limitations](#known-limitations).

## What it does

Real-world GIS data often has problems:

- Self-intersecting rings ("bowties")
- Unclosed rings
- Holes outside their shell
- Consecutive duplicate coordinates
- NaN or infinite coordinates
- Collapsed/degenerate polygons (zero area, colinear)

`geo-repair` detects these problems (custom OGC-style validation) and fixes them — picking
different algorithms per geometry type:

| Geometry | What happens |
|----------|-------------|
| `Polygon` / `MultiPolygon` | Two repair strategies, selected by [`PolyMethod`](https://docs.rs/geo-repair/latest/geo_repair/enum.PolyMethod.html) |
| `LineString` / `MultiLineString` | Self-intersection noding, NaN filtering, duplicate removal |
| `Line` | Zero-length and NaN detection |
| `Point` / `MultiPoint` | NaN/infinite coordinate filtering |
| `Rect` / `Triangle` | Basic degeneracy checks |
| `GeometryCollection` | Recursive repair of children |

## Polygon repair strategies

The two polygon algorithms work differently:

| Strategy | Approach | Strengths | Weaknesses |
|----------|----------|-----------|------------|
| **Arrange** (CDT) | Constrained Delaunay triangulation → face labeling → ring extraction ([Ledoux et al. 2014](https://doi.org/10.1016/j.cageo.2014.01.009)) | Handles any topology. No self-intersection limit. | Slower, especially on large rings. Requires `spade`. |
| **Structure** (fast path) | Planar graph extraction → face walking → winding-number assembly | 10–100× faster for valid/simple inputs. No external deps. | Falls back on complex topologies (many holes, nested self-intersections). |

**Auto** (default) tries Structure first, then falls back to Arrange.

## Performance

Single-polygon timings (i5-12400F, structure fast path):

| Benchmark           | geo-repair | GEOS    |
|---------------------|------------|---------|
| square (4v)         | 0.64 µs    | 2.12 µs |
| bowtie (4v)         | 12.4 µs    | 106 µs  |
| complex bowtie (10v)| 26.9 µs    | 615 µs  |
| large 100v          | 10.4 µs    | 27.3 µs |
| large 500v          | 117 µs     | 321 µs  |
| large 2000v         | 610 µs     | 3.21 ms |
| large 5000v         | 3.28 ms    | 16.3 ms |
| large 10000v        | 16.3 ms    | 62.1 ms |
| overlapping mpoly   | 3.33 µs    | 237 µs  |

Real-world data benchmark (1,578,988 polygons, structure parallel batch):

**Full dataset (1,578,988 polygons):**

|            | total   | per-poly  | speedup |
|------------|---------|-----------|---------|
| geo-repair | 5.02 s  | 3.2 µs    | —       |
| GEOS       | 18.79 s | 11.9 µs   | 3.7×    |

**Invalid subset (1,848 polygons):**

|            | total   | per-poly  | speedup |
|------------|---------|-----------|---------|
| geo-repair | 3.21 s  | 1.74 ms   | —       |
| GEOS       | 10.12 s | 5.48 ms   | 3.2×    |

Quick sweep (structure, parallel batch):

| Benchmark        | geo-repair | GEOS     |
|------------------|------------|----------|
| valid 4v         | 0.214 µs   | 11.27 µs |
| invalid bowtie   | 3.805 µs   | 308 µs   |
| invalid star 100v| 11.66 µs   | 513 µs   |


Run yourself:

```shell
# Quick sweep (default bench, no GEOS)
cargo bench

# Criterion benchmarks (requires bench-criterion)
cargo bench --features bench-criterion --bench criterion

# Real-world dataset (includes bundled GEOS via bench-geos feature)
$env:BENCH_FILE = "path/to/data.bin"
cargo bench --features bench-geos --bench real_world
```

GEOS is bundled via [`geos-src`](https://crates.io/crates/geos-src) and compiled
statically with MSVC optimization fixes — no system install needed. Requires
CMake and a C++ compiler.

### Parallelism

Use the `parallel` feature (enabled by default) for multi-core polygon repair.
Uses `rayon` to spread polygons across worker threads. Parallelism is **per-polygon** — each
polygon is still single-threaded.

The following are parallelized:
- `par_fix_polygon_batch` — batch polygon repair
- `par_make_valid` / `par_make_valid_with_config` — trait methods on multi-geometry types
- `MultiPolygon`, `MultiLineString`, `MultiPoint`, `GeometryCollection` — each child is repaired in parallel
- Validation pre-scan in the real-world benchmark

### SIMD

Use the `simd` feature (enabled by default) for AVX2-accelerated orientation tests:

- `orient2d_batch` — processes 4 orientation tests at once (256-bit vectors)
- `is_ring_ccw_simd` — batch winding detection
- `point_in_ring_exclusive` — AVX2-accelerated winding-number point-in-ring test

Roughly 1.5–3× faster on large rings (≥100 vertices) than scalar iteration. When coordinates
are near-collinear or extreme, it falls back to Shewchuk adaptive-precision arithmetic (the
`robust` crate).

The following are SIMD-accelerated:
- Ring winding direction detection
- Point-in-ring containment tests (hole classification)
- Batch orientation tests in the CDT flow

## Known limitations

### Correctness

- **Structure fast path may produce invalid output** on polygons with complex hole nesting
  or specific self-touching patterns. When `bench-geos` is enabled, the benchmark checks
  results against GEOS; on production data, you should verify.
- **CDT arranger may panic** on certain degenerate inputs (all-collinear exterior rings,
  coordinates near `f64::MAX`). This is a known problem with `spade`.
- **OGC compliance is not guaranteed.** The validation module checks OGC predicates (ring
  closure, self-intersection, hole containment, orientation), but repair output is not
  formally verified against the OGC Simple Features spec.
- **2D only.** Z and M coordinates are stripped during processing.

### Performance

- **Large polygons (10k+ vertices)** are expensive in both modes. Structure has an O(n²)
  intersection check in the worst case. Consider simplifying or tiling very large polygons first.
- **Hole-heavy polygons** (50+ holes) stress the structure algorithm's classification phase.
  The O(n²) containment checks between holes eat runtime.
- **No streaming.** All geometry is loaded into memory. Multi-gigabyte files will
  exhaust RAM.

### Portability

- `simd` requires `avx2` (x86-64, Haswell or newer). ARM NEON and WASM SIMD are not supported.
- `arrange` requires `spade`, which has its own constraints (no WASM).
- Windows needs the GEOS DLL on PATH for `bench-geos`.

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `arrange` | CDT-based polygon repair (requires `spade`) | yes |
| `structure` | Structure-based fast path repair | yes |
| `parallel` | Rayon parallel processing | yes |
| `simd` | AVX2-accelerated orientation tests | yes |
| `io-geojson` | GeoJSON load/export | yes |
| `io-wkt` | WKT load/export | yes |
| `io-wkb` | WKB load/export | no |
| `io-csv` | CSV+WKT column load | no |
| `serde` | Geometry serde support | no |
| `ffi` | C-compatible FFI API (implies `io-wkb`) | no |
| `bench-geos` | GEOS comparison benchmarks | no |
| `load-shp` | Shapefile loading | no |

## FFI / Python

Enable the `ffi` feature for a C-compatible API using WKB:

```c
GeoRepairResult result = geo_repair_make_valid(wkb_data, wkb_len);
geo_repair_free_result(result);
```

Meant for QGIS Python scripts and PyO3/maturin bindings.

## Ecosystem

- Uses `geo` 0.33 types natively
- Re-exports `geo::MakeValid` as `GeoMakeValid`
- Optional `serde` support
- Standard georust format crates: `geojson`, `wkt`, `wkb`, `shapefile`

## License

Apache-2.0

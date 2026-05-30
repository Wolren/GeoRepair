# geo-repair

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.95+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)

**Fix invalid GIS geometries** - detects and repairs broken polygons, lines, and points.

> **⚠️ EXPERIMENTAL - NOT PRODUCTION-READY**
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

`geo-repair` detects these problems (custom OGC-style validation) and fixes them - picking
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

### Real-world dataset (1,578,988 polygons)

Structure parallel batch on a production GIS dataset. GEOS setup (WKT→geom): +32 s.

| Dataset (1.58M polys) | geo-repair | GEOS | vs GEOS |
|-----------------------|------------|------|---------|
| Fast path / valid poly | **52 µs** | - | - |
| Invalid subset (1848) | **2.5 s** total / **1.35 ms** ea | **9.8 s** total / **5.31 ms** ea | **3.9×** |
| Full dataset | **4.2 s** total / **2.6 µs** ea | **26.0 s** total / **16.5 µs** ea | **6.2×** |

### Synthetic benchmarks (parallel, grid+R-tree hybrid)

| Benchmark | geo-repair (parallel) | GEOS | vs GEOS |
|-----------|----------------------|------|---------|
| Valid polygon 4v | **0.15 µs** | **4.4 µs** | **29×** |
| Valid polygon 100v | **0.86 µs** | **48.6 µs** | **57×** |
| Valid polygon 10k | **88 µs** | **4472 µs** | **51×** |
| Invalid bowtie 4v | **1.9 µs** | **91 µs** | **49×** |
| Invalid star 100v | **7.5 µs** | **103 µs** | **14×** |
| Collinear ls 500v | **18 µs** | **84.7 µs** | **5×** |
| Hilbert curve 1024v | **175 µs** | **203 µs** | **1.2×** |
| Lissajous 1000v | **333 µs** | **486 µs** | **1.5×** |
| Star-burst 500sp | **753 µs** | **357 µs** | GEOS **2.1×** |
| Spoke wheel 500sp | **862 µs** | **366 µs** | GEOS **2.4×** |


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
statically with MSVC optimization fixes - no system install needed. Requires
CMake and a C++ compiler.

### Parallelism

Use the `parallel` feature (enabled by default) for multi-core polygon repair via `rayon`.
Two levels of parallelism:

**Batch level** - spreads independent polygons across worker threads:
- `par_fix_polygon_batch` - batch polygon repair
- `par_make_valid` / `par_make_valid_with_config` - trait methods on multi-geometry types
- `MultiPolygon`, `MultiLineString`, `MultiPoint`, `GeometryCollection` - each child in parallel

**Intra-polygon** - parallel hot loops inside a single polygon's repair:
- Structure hole fixing (`structure/mod.rs`)
- Structure parent-of / nesting resolution (`structure/mod.rs`)
- Hole containment classification (`classify.rs`)
- Grid-cell edge-edge intersection testing (`fix_ring.rs`, >500 cells)
- Monotone-chain self-intersection check (`arrange/prep.rs`, ≥200 chains)

The Arrange (CDT) path has limited intra-polygon parallelism (monotone chains only). Structure has the most breadth. Batch-level parallelism is always additive - no oversubscription concern because the intra-polygon loops only fire for large inputs, while the batch path uses the same global `rayon` thread pool.

### SIMD

Use the `simd` feature (enabled by default) for AVX2-accelerated orientation tests:

- `orient2d_batch` - processes 4 orientation tests at once (256-bit vectors)
- `is_ring_ccw_simd` - batch winding detection
- `point_in_ring_exclusive` - AVX2-accelerated winding-number point-in-ring test

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

- **Large polygons (10k+ vertices)** are expensive in both modes. Structure uses an R-tree
  (O(n log n) expected) for intersection detection, but worst-case radial geometries
  (e.g., star-bursts, spoke wheels) still generate O(n²) candidate pairs.
  Consider simplifying or tiling very large polygons first.
- **Hole-heavy polygons** (50+ holes) stress the structure algorithm's classification phase.
  The containment checks between holes are accelerated with an R-tree (O(n log n) expected).
- **Streaming via chunked API.** `par_fix_polygon_batch_chunked` processes an
  iterator in fixed-size batches, bounding peak memory to `chunk_size` polygons.
  Pair with `load_shp_stream` or `load_bin_stream` for lazy file reading - only
  one chunk is in memory at a time.

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

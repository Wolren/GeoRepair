# GeoRepair

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.95+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)

**Fix invalid GIS geometries** — detects and repairs broken polygons, lines, and points
with per-type optimal algorithms. Aims for **OGC Simple Features compliance**.

## Getting started

```toml
[dependencies]
geo-repair = "0.1"
```

Minimal load → validate → repair → export pipeline:

```rust
use geo_repair::{MakeValid, GeoValidation, load_geometries, export_geometries};

let geoms = load_geometries("input.geojson")?;                        // load
let valid: Vec<_> = geoms.iter().all(|g| g.is_valid());               // quick check
let fixed: Vec<_> = geoms.iter().map(|g| g.make_valid()).collect();   // repair
export_geometries(&fixed, "output.geojson")?;                          // export
```

Detailed validation with repair:

```rust
use geo_repair::{MakeValid, GeoValidation, ValidateAndFix};

// Validate and fix in one step (GEOS-compatible pipeline)
let (result, fixed) = polygon.validate_and_fix();
if !result.valid {
    for err in &result.errors {
        eprintln!("  Violation: {err}");
    }
}

// Fix only if invalid, return Result
match polygon.validate_or_fix() {
    Ok(g) => { /* polygon was valid or fix succeeded */ }
    Err((errors, _)) => { /* still invalid despite repair */ }
}
```

## What it does

Real-world GIS data often has problems:

- Self-intersecting rings ("bowties")
- Unclosed rings
- Holes outside their shell
- Consecutive duplicate coordinates
- NaN or infinite coordinates
- Collapsed/degenerate polygons (zero area, colinear)

`geo-repair` detects these problems (OGC-style validation) and fixes them, picking
different algorithms per geometry type:

| Geometry | What happens |
|----------|-------------|
| `Polygon` / `MultiPolygon` | Two repair strategies, selected by [`PolyMethod`](https://docs.rs/geo-repair/latest/geo_repair/enum.PolyMethod.html) |
| `LineString` / `MultiLineString` | Self-intersection noding, NaN filtering, duplicate removal |
| `Line` | Zero-length and NaN detection |
| `Point` / `MultiPoint` | NaN/infinite coordinate filtering, duplicate removal |
| `Rect` / `Triangle` | Basic degeneracy checks |
| `GeometryCollection` | Recursive repair of children |

## Validation (OGC Simple Features)

The [`GeoValidation`](https://docs.rs/geo-repair/latest/geo_repair/trait.GeoValidation.html) trait checks
18 OGC validity rules:

| Rule | Applies to |
|------|-----------|
| Coordinate finiteness | All geometries |
| Ring closure | Polygon rings |
| Ring minimum vertices (≥4) | Polygon rings |
| Ring self-intersection | Polygon rings |
| Pinch points (non-consecutive duplicates) | Rings |
| Hole containment (inside shell) | Polygon |
| No nested holes | Polygon |
| Interior ring connectivity | Polygon |
| Ring orientation (exterior CCW, interior CW) | Polygon |
| Non-collinear rings | Polygon |
| Consecutive duplicates | Lines/rings |
| Duplicate rings | Polygon |
| Duplicate points | MultiPoint |
| Duplicate lines | MultiLineString |
| Non-zero-length lines | Line |
| Non-degenerate exterior | Polygon |
| Simplicity (no interior intersections) | LineString, MultiLineString |
| Nesting depth limit | GeometryCollection |

```rust
use geo_repair::{GeoValidation, is_valid, validate};

// Boolean check
if !geom.is_valid() { /* ... */ }

// Convenience functions
if !is_valid(&geom) { /* ... */ }
let result = validate(&geom);
println!("Errors: {:?}", result.errors);
```

## Polygon repair strategies

The two polygon algorithms work differently:

| Strategy | Approach | Strengths | Weaknesses |
|----------|----------|-----------|------------|
| **Arrange** (CDT) | Constrained Delaunay triangulation → face labeling → ring extraction ([Ledoux et al. 2014](https://doi.org/10.1016/j.cageo.2014.01.009)) | Handles any topology. No self-intersection limit. | Slower, especially on large rings. Requires `spade`. |
| **Structure** (fast path) | Planar graph extraction → face walking → winding-number assembly | 10–100× faster for valid/simple inputs. No external deps. | Falls back on complex topologies (many holes, nested self-intersections). |

**Auto** (default) tries Structure first, then falls back to Arrange.

### Repair configuration

```rust
use geo_repair::{MakeValidConfig, PolyMethod, MakeValid};

let config = MakeValidConfig {
    poly_method: PolyMethod::Arrange,
    keep_collapsed: false,
    ..Default::default()
};
let fixed = geom.make_valid_with_config(&config);
```

## Coordinate Reference Systems (CRS)

The [`Crs`](https://docs.rs/geo-repair/latest/geo_repair/struct.Crs.html) type stores CRS metadata
and can be attached to geometries or features for round-tripping through I/O formats
that support it (GeoJSON foreign members, WKT, Shapefile, GeoPackage).

```rust
use geo_repair::Crs;

let crs = Crs::from_epsg(4326);
let fixed = geom.make_valid_with_config(&MakeValidConfig {
    crs: Some(crs),
    ..Default::default()
});
```

CRS-aware tolerance heuristics:
- Geographic (lon/lat): `1e-10` degrees
- Projected (metres): `1e-6` metres
- Unknown: `1e-12`

The `target_crs` field on [`MakeValidConfig`](https://docs.rs/geo-repair/latest/geo_repair/struct.MakeValidConfig.html)
enables post-repair CRS transformation when the `proj` feature is available (not yet backed by a PROJ dependency).

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

# Real-world dataset (GEOS comparison via bench-geos feature)
./scripts/fetch-geos-src.sh               # fetch geos-src crate (once)
scripts/bench-geos.ps1                    # Windows — auto-detects conda GEOS
# or manually:
$env:BENCH_FILE = "benches/real_world/data_0.bin"
cargo bench --features bench-geos --bench real_world
```

> **Tip**: Use `scripts/bench-geos.ps1` (Windows) or `scripts/bench-geos.sh` (Unix) to
> automatically detect and configure GEOS before running the benchmark.

#### GEOS setup

The GEOS C++ source tree is **not vendored** in the repository. Before running
GEOS benchmarks, you must fetch the `geos-src` crate first:

```shell
# Bash (Linux/macOS/Git Bash on Windows)
./scripts/fetch-geos-src.sh

# PowerShell (Windows)
.\scripts\fetch-geos-src.ps1
```

This downloads the `geos-src` crate (v0.2.4 by default) into `patches/geos-src/`
so the build system can compile the GEOS C++ library for benchmark comparison.

Requires GEOS installed on the system (for benchmark comparisons only).

**Windows (conda)** — install, then use the script or set paths manually:

```shell
conda install -c conda-forge geos
.\scripts\fetch-geos-src.ps1
.\scripts\bench-geos.ps1
```

**Linux/macOS** — use your system package manager:

```shell
# Debian/Ubuntu
sudo apt install libgeos-dev
./scripts/fetch-geos-src.sh
./scripts/bench-geos.sh
# macOS (Homebrew)
brew install geos
./scripts/fetch-geos-src.sh
./scripts/bench-geos.sh
```

> **Note**: The `fetch-geos-src` script is idempotent — it's safe to run multiple
> times. It skips the download if `patches/geos-src/` already contains a valid
> `geos-src` installation.

### Parallelism

Use the `parallel` feature (enabled by default) for multi-core polygon repair via `rayon`.
Two levels of parallelism:

**Batch-level** — spreads independent polygons across worker threads:
- `par_fix_polygon_batch` — batch polygon repair
- `par_make_valid` / `par_make_valid_with_config` — trait methods on multi-geometry types
- `MultiPolygon`, `MultiLineString`, `MultiPoint`, `GeometryCollection` — each child in parallel

**Intra-polygon** — parallel hot loops inside a single polygon's repair:
- Structure hole fixing (`structure/mod.rs`)
- Structure parent-of / nesting resolution (`structure/mod.rs`)
- Hole containment classification (`classify.rs`)
- Grid-cell edge-edge intersection testing (`fix_ring.rs`, >500 cells)
- Monotone-chain self-intersection check (`arrange/prep.rs`, ≥200 chains)

The Arrange (CDT) path has limited intra-polygon parallelism (monotone chains only). Structure
has the most breadth. Batch-level parallelism is always additive — no oversubscription concern
because the intra-polygon loops only fire for large inputs, while the batch path uses the same
global `rayon` thread pool.

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

### Streaming via chunked API

`par_fix_polygon_batch_chunked` processes an iterator in fixed-size batches, bounding peak
memory to `chunk_size` polygons. Pair with `load_shp_stream` or `load_bin_stream` for lazy
file reading — only one chunk is in memory at a time.

## I/O format support

| Format | Load | Export | Features | Z/M | CRS |
|--------|------|--------|----------|-----|-----|
| **GeoJSON** (.geojson/.json) | ✓ | ✓ | ✓ | ✓ | ✓ |
| **WKT** (.wkt) | ✓ | ✓ | — | — | ✓ |
| **WKB** (.wkb) | ✓ | ✓ | ✓ | ✓ | ✓ |
| **CSV+WKT** (.csv) | ✓ | ✓ | — | — | — |
| **Shapefile** (.shp) | ✓ | ✓ | ✓ | — | ✓ |
| **GeoPackage** (.gpkg) | ✓ | ✓ | ✓ | ✓ | ✓ |
| **GML** (.gml/.xml) | ✓ | ✓ | ✓ | — | ✓ |
| **Binary** (.bin) | ✓ | ✓ | — | — | — |

Format auto-detection via file extension. Load with `load_geometries`, export with
`export_geometries`. Feature-attribute-persisting variants (`load_features`,
`export_features`) also available.

### Feature attributes

```rust
use geo_repair::{load_features, export_features, MakeValid};

let mut features = load_features("input.geojson")?;
for f in &mut features {
    f.geometry = f.geometry.make_valid();
}
export_features("output.geojson", &features)?;
```

### Z and M coordinates

Z and M values are preserved through load/repair/export when using the
[`ZmGeometry`](https://docs.rs/geo-repair/latest/geo_repair/zm/struct.ZmGeometry.html) API
or feature-level I/O functions. The core `make_valid` pipeline operates on 2D coordinates
only; Z/M are carried alongside and re-merged on export.

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
| `io-gpkg` | GeoPackage load/export (requires SQLite) | no |
| `io-gml` | GML load/export (requires XML parser) | no |
| `io-all` | Enables all I/O formats | no |
| `proj` | CRS transformation support (placeholder) | no |
| `serde` | Geometry serde support | no |
| `ffi` | C-compatible FFI API (implies `io-wkb`) | no |
| `python` | Python bindings via PyO3 + maturin | no |
| `bench-geos` | GEOS comparison benchmarks | no |
| `load-shp` | Shapefile loading | no |

## Python bindings

```bash
pip install geo_repair          # from PyPI (once published)
# or build from source:
pip install maturin
python -m maturin build --features python
pip install target/wheels/geo_repair-*.whl
```

```python
import geo_repair

# WKT input/output
geo_repair.repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")
# -> 'MULTIPOLYGON(((0 5,0 0,2.5 2.5,0 5)),((5 5,2.5 2.5,5 0,5 5)))'

# GeoJSON input/output (Feature, FeatureCollection, or bare Geometry)
geo_repair.repair_geojson('{"type":"Polygon","coordinates":...}')

# Choose method (auto, arrange, structure)
geo_repair.repair_wkt("...", "structure")

# Validate only — no repair
geo_repair.is_valid_wkt("POINT(1 2)")     # -> True
geo_repair.validate_wkt("POLYGON(...)")   # -> ['Ring has self-intersections']
```

## C FFI

Enable the `ffi` feature for a C-compatible API using WKB:

```c
#include "geo_repair.h"

GeoRepairResult result = geo_repair_make_valid(wkb_data, wkb_len);
if (result.success) {
    // use result.wkb_data / result.wkb_len
}
geo_repair_free_result(&result);
```

The `GeoRepairResult` struct and function signatures are in `src/bindings/ffi.rs`.

## Known limitations

### Correctness

- **CDT arranger may panic** on certain degenerate inputs (all-collinear exterior rings,
  coordinates near `f64::MAX`). This is a known problem with `spade`.
- **OGC compliance is a key goal but not yet formally certified.** The validation module
  checks 18 OGC predicates. The repair module aims to produce OGC-valid output but is not
  formally verified against the full Simple Features spec. Missing rules tracked on GitHub.
- **GeometryCollection cross-component intersection** is not validated.
- **Z/M coordinate consistency** is not validated.

### Performance

- **Large polygons (10k+ vertices)** are expensive in both modes. Structure uses an R-tree
  (O(n log n) expected) for intersection detection, but worst-case radial geometries
  (e.g., star-bursts, spoke wheels) still generate O(n²) candidate pairs.
  Consider simplifying or tiling very large polygons first.
- **Hole-heavy polygons** (50+ holes) stress the structure algorithm's classification phase.
  The containment checks between holes are accelerated with an R-tree (O(n log n) expected).

### Portability

| Platform | Core¹ | SIMD | I/O² | Parallel | Python |
|----------|-------|------|------|----------|--------|
| x86_64 Windows/Linux/macOS | ✓ | ✓ AVX2³ | ✓ | ✓ | ✓ |
| aarch64 macOS/Linux | ✓ | scalar | ✓ | ✓ | ✓ |
| WASM32 | ✓ | scalar | ✗ | ✗ | ✗ |

¹ `arrange` + `structure` features. ² `io-*` features (GeoJSON, WKT, WKB, etc.).
³ Enable with `RUSTFLAGS="-C target-cpu=native"` at build time.
Fallback to scalar on CPUs without AVX2 or non-x86_64 targets.

- `bench-geos` requires a system GEOS installation (conda recommended).

## Ecosystem

- Uses `geo` 0.33 types natively
- Re-exports `geo::MakeValid` as `MakeValid`
- Optional `serde` support
- Standard georust format crates: `geojson`, `wkt`, `wkb`, `shapefile`, `rusqlite`, `quick-xml`

## License

Apache-2.0

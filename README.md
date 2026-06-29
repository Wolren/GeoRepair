# GeoRepair

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.95+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)

OGC geometry repair and validation for Rust.  Detects and fixes invalid
GIS geometries (self-intersections, unclosed rings, degenerate shapes, NaN
coordinates, and more) using algorithms selected by geometry type.

- **Polygons:** CDT arrangement (Arrange) or planar-graph structure fast path (Structure)
- **Lines:** Self-intersection noding with snap-rounding fallback
- **Points:** NaN/Inf filtering, deduplication
- **Validation:** 18 OGC Simple Features predicates, Shewchuk adaptive-precision arithmetic
- **Correctness:** 2490/2490 GEOS XML validation suite passing

## Contents

- [Quick start](#quick-start)
- [What it does](#what-it-does)
- [Validation](#validation)
- [Polygon repair strategies](#polygon-repair-strategies)
- [CRS support](#crs-support)
- [Performance](#performance)
- [I/O](#io)
- [Features](#features)
- [Python bindings](#python-bindings)
- [C FFI](#c-ffi)
- [Known limitations](#known-limitations)

## Quick start

```toml
[dependencies]
geo-repair = "0.2"
```

**Validate and repair geometries:**

```rust
use geo_repair::{is_valid, validate, MakeValid, ValidateAndFix};

// Check validity
let result = validate(&geom);
if !result.valid {
    for err in &result.errors {
        eprintln!("  {err}");
    }
}

// Fix invalid geometry
let fixed = geom.make_valid();

// Combined validate-and-fix (returns result + fixed geometry)
let (result, fixed) = geom.validate_and_fix();
```

**With method selection:**

```rust
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

let config = MakeValidConfig {
    poly_method: PolyMethod::Arrange,
    keep_collapsed: false,
    ..Default::default()
};
let fixed = geom.make_valid_with_config(&config);
```

**WKB I/O (built-in, no dependencies):**

```rust
use geo_repair::{read_wkb, write_wkb, read_wkb_concat};

// Encode geometry to WKB bytes
let wkb: Vec<u8> = write_wkb(&geom);

// Decode single geometry from WKB
let geom = read_wkb(&wkb).unwrap();

// Decode concatenated multi-geometry WKB
let geoms: Vec<Geometry<f64>> = read_wkb_concat(&concat_buffer).unwrap();
```

**Binary format loading (custom `.bin` format, fast bulk I/O):**

```rust
use geo_repair::load_bin;

// Load polygons from custom binary format
let polys: Vec<Polygon<f64>> = load_bin("dataset.bin").unwrap();
```

## What it does

Real-world GIS data often contains geometry defects.  GeoRepair detects
these via OGC-style validation and repairs them:

| Geometry | Repair approach |
|----------|----------------|
| `Polygon` / `MultiPolygon` | Two strategies, selected by [`PolyMethod`](https://docs.rs/geo-repair/latest/geo_repair/enum.PolyMethod.html) |
| `LineString` / `MultiLineString` | NaN filtering, duplicate removal, self-intersection noding |
| `Line` | Zero-length and NaN detection |
| `Point` / `MultiPoint` | NaN/Inf filtering, deduplication |
| `Rect` / `Triangle` | Basic degeneracy checks |
| `GeometryCollection` | Recursive repair of children |

## Validation

The [`GeoValidation`](https://docs.rs/geo-repair/latest/geo_repair/trait.GeoValidation.html)
trait checks 18 OGC validity rules using Shewchuk adaptive-precision
orientation tests (via the `robust` crate) for reliable results near
degeneracies.

| Rule | Applies to |
|------|-----------|
| Coordinate finiteness | All geometries |
| Ring closure | Polygon rings |
| Ring minimum vertices (>=4) | Polygon rings |
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

**Usage:**

```rust
use geo_repair::{is_valid, validate, validate_reason, GeoValidation, ValidationResult};

let ok: bool = geom.is_valid();
let ok2: bool = is_valid(&geom);         // free function, no trait import needed

let result: ValidationResult = validate(&geom);
let reason: String = validate_reason(&geom);
```

## Polygon repair strategies

| Strategy | Approach | Strengths | Weaknesses |
|----------|----------|-----------|------------|
| **Arrange** (CDT) | Constrained Delaunay triangulation -> face labeling -> ring extraction (Ledoux et al. 2014) | Handles any topology. No self-intersection limit. | Slower, especially on large rings. Requires `spade`. |
| **Structure** (fast path) | Planar graph extraction -> face walking -> winding-number assembly | 10-100x faster for valid/simple inputs. No external deps. | Falls back on complex topologies (many holes, nested self-intersections). |

**Auto** (default) tries Structure first and falls back to Arrange.

The repair pipeline enforces OGC-correct winding order (CCW exterior, CW
interior) on all output.

## CRS support

The [`Crs`](https://docs.rs/geo-repair/latest/geo_repair/struct.Crs.html)
type stores EPSG codes and provides CRS-aware tolerance heuristics:

- Geographic (lon/lat): `1e-10` degrees
- Projected (metres): `1e-6` metres
- Unknown: `1e-12`

CRS is not tied to I/O — you set it directly on `MakeValidConfig`:

```rust
use geo_repair::{Crs, MakeValidConfig};

let config = MakeValidConfig {
    crs: Some(Crs::from_epsg(4326)),
    ..Default::default()
};
```

For CRS-tagged I/O (GeoJSON, Shapefile, GPKG), use GEOS bindings or
other external tools to load data and extract CRS separately.

## Performance

### GEOS XML validation suite

The validation module passes 2490 out of 2490 tests from the GEOS XML
test suite, covering all OGC Simple Features validity predicates.

### Real-world dataset (1,578,988 polygons)

Structure parallel batch on a production GIS dataset.  GEOS geometries
built from CoordSeq (no WKT overhead, ~1.4 s setup).  i5-12400F (6C/12T).

| Dataset | geo-repair | GEOS (parallel) | Ratio |
|---------|------------|-----------------|-------|
| Invalid subset (1855 polys) | **2.25 s** / 1.21 ms each | **2.10 s** / 1.13 ms each | 1.07x |
| Full dataset | **3.76 s** / 2.4 us each | **3.67 s** / 2.3 us each | 1.03x |

### Synthetic benchmarks (parallel, grid+R-tree hybrid)

| Benchmark | geo-repair | GEOS | Ratio |
|-----------|------------|------|-------|
| Valid polygon 4v | 0.15 us | 4.4 us | 29x |
| Valid polygon 100v | 0.86 us | 48.6 us | 57x |
| Valid polygon 10k | 88 us | 4472 us | 51x |
| Invalid bowtie 4v | 1.9 us | 91 us | 49x |
| Invalid star 100v | 7.5 us | 103 us | 14x |
| Collinear ls 500v | 18 us | 84.7 us | 5x |
| Hilbert curve 1024v | 175 us | 203 us | 1.2x |
| Lissajous 1000v | 333 us | 486 us | 1.5x |
| Star-burst 500sp | 753 us | 357 us | GEOS 2.1x |
| Spoke wheel 500sp | 862 us | 366 us | GEOS 2.4x |

### Run benchmarks

```shell
# Quick sweep
cargo bench

# Real-world dataset with GEOS comparison
scripts/bench-geos.ps1              # Windows (conda)
scripts/bench-geos.sh               # Linux/macOS

# Criterion benchmarks
cargo bench --features bench-criterion --bench criterion
```

### Internal optimizations

- **R-tree spatial indexing** for intersection detection in ring validation
  and snap-rounding (O(log h) per segment vs O(h)).
- **Rotation-invariant ring fingerprints** for O(1) duplicate ring detection.
- **O(n) BFS topological sort** replaces O(n^2) depth propagation.
- **DD (double-double) arithmetic** for 106-bit precision in critical
  intersection computations.
- **GeometryPrecisionReducer** as last-resort fallback with progressive
  coarsening (1e-10, 1e-8, 1e-6, 1e-4).
- **Parallelism:** Batch-level (rayon, independent polygons) plus
  intra-polygon hot loops (hole fixing, edge-edge tests, etc.).
- **SIMD:** AVX2-accelerated orientation tests (1.5-3x on rings >= 100
  vertices), with Shewchuk fallback for near-collinear cases.

## I/O

GeoRepair provides minimal built-in I/O — a custom binary format for bulk
polygon loading and a zero-dependency WKB parser/encoder.  All other
format handling (GeoJSON, Shapefile, GeoPackage, GML, CSV, WKT) is left to
external tools such as GEOS bindings, GDAL, or dedicated conversion
scripts.

| Format | Load | Export | Description |
|--------|------|--------|-------------|
| **Binary** (.bin) | `load_bin()` / `load_bin_stream()` | — | Custom format, bulk polygon I/O, ~950 MB/s throughput |
| **WKB** (.wkb) | `read_wkb()` / `read_wkb_concat()` | `write_wkb()` | Standard OGC WKB, LE/BE, EWKB SRID |

**SHP to .bin conversion:**

```bash
python scripts/convert_shp_to_bin.py input.shp output.bin
```

**WKB roundtrip example:**

```rust
use geo_repair::{read_wkb, write_wkb};

let bytes: Vec<u8> = write_wkb(&geometry);
let geom = read_wkb(&bytes).unwrap();
```

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `arrange` | CDT-based polygon repair (requires `spade`) | yes |
| `structure` | Structure-based fast path repair | yes |
| `parallel` | Rayon parallel processing | yes |
| `simd` | AVX2-accelerated orientation tests | yes |
| `simd-portable` | Portable SIMD via `core::simd` | no |
| `validate` | OGC validation predicates (enabled by default, can disable for smaller builds) | yes |
| `proj` | CRS transformation (placeholder) | no |
| `serde` | Geometry serde support | no |
| `ffi` | C-compatible FFI | no |
| `python` | Python bindings (PyO3) | no |
| `bench-geos` | GEOS comparison benchmarks | no |
| `bench-criterion` | Criterion benchmark harness | no |
| `mimalloc` | Use mimalloc global allocator | no |

## Python bindings

Build and install:

```bash
# from source:
pip install maturin
python -m maturin build --features python
pip install target/wheels/geo_repair-*.whl

# or once published:
pip install geo_repair
```

The Python package exposes WKB-based batch processing — ideal for use
with QGIS or GDAL where WKB is the native geometry format.

```python
import geo_repair
import struct

# Single geometry repair (WKB in, WKB out)
wkb_in = bytes(...)  # from QgsGeometry.asWkb() or similar
wkb_out = geo_repair.repair_wkb(wkb_in, method="auto")

# Batch repair (sequential)
wkb_batch = [wkb_1, wkb_2, ...]
results = geo_repair.repair_wkb_batch(wkb_batch, method="structure")

# Batch repair (parallel — requires `parallel` feature)
results = geo_repair.par_repair_wkb_batch(wkb_batch, method="auto")

# Combined repair + validation batch
# Returns list of (repaired_wkb, valid_before, error_list)
results = geo_repair.repair_validate_wkb_batch(wkb_batch, method="auto")
for out_wkb, was_valid, errors in results:
    if not was_valid:
        print("  Errors:", errors)
    if out_wkb:
        set_geometry(result_feature, out_wkb)
```

**QGIS integration:** See `qgis/qgis_geo_repair.py` for a complete
processing script that iterates features, batches WKBs, and sends them
to this engine.

## C FFI

Enable the `ffi` feature for a C-compatible API using WKB:

```c
#include <stdint.h>
#include <stdbool.h>

typedef struct {
    bool      success;
    uint8_t*  wkb_data;
    size_t    wkb_len;
    char*     error_msg;
} GeoRepairResult;

GeoRepairResult geo_repair_make_valid(const uint8_t* wkb_data, size_t wkb_len);
GeoRepairResult geo_repair_make_valid_with_config(
    const uint8_t* wkb_data, size_t wkb_len,
    bool keep_collapsed, uint8_t poly_method);
GeoRepairResult geo_repair_make_valid_with_config_full(
    const uint8_t* wkb_data, size_t wkb_len,
    bool keep_collapsed, uint8_t poly_method,
    uint8_t fill_rule, int32_t epsg_code);
uint8_t         geo_repair_is_valid(const uint8_t* wkb_data, size_t wkb_len);
GeoRepairResult geo_repair_validate_reason(const uint8_t* wkb_data, size_t wkb_len);
void            geo_repair_free_result(GeoRepairResult* result);
```

## Known limitations

### Correctness

- **CDT arranger may panic** on certain degenerate inputs (all-collinear
  exterior rings, coordinates near f64::MAX).  This is a known limitation
  of `spade`.
- **OGC compliance** is a key goal but not yet formally certified.  The
  validation module checks 18 OGC predicates and passes 2490/2490 GEOS XML
  tests.  The repair module aims to produce OGC-valid output but is not
  formally verified against the full Simple Features specification.
- **GeometryCollection cross-component intersection** is not validated.
- **Z/M coordinate consistency** is not validated.

### Performance

- **Large polygons (10k+ vertices)** are expensive in both modes.
  Consider simplifying or tiling very large polygons first.
- **Hole-heavy polygons (50+ holes)** stress the structure algorithm's
  classification phase.  R-tree acceleration provides O(n log n) expected
  performance.

### Portability

| Platform | Core | SIMD | I/O | Parallel | Python |
|----------|------|------|-----|----------|--------|
| x86_64 Windows/Linux/macOS | Yes | Yes (AVX2) | Yes | Yes | Yes |
| aarch64 macOS/Linux | Yes | Scalar | Yes | Yes | Yes |
| WASM32 | Yes | Scalar | Yes | No | No |

AVX2 requires `RUSTFLAGS="-C target-cpu=native"` at build time.  Falls
back to scalar on CPUs without AVX2 or non-x86_64 targets.

## License

Apache-2.0

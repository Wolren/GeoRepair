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

OGC geometry repair and validation for Rust.  Detects and fixes invalid
GIS geometries (self-intersections, unclosed rings, degenerate shapes, NaN
coordinates, and more) using algorithms selected by geometry type.

The **Structure** strategy (default) mirrors GEOS's ST_MakeValid
algorithm: planar graph extraction, face walking, and winding-number
assembly.  The **Arrange** strategy uses CDT-based repair as a robust
fallback for complex topologies.  Passes 2490/2490 GEOS XML validation
tests, with parallel batch performance **0.30× GEOS** (3.3× faster) on
1.58M real-world polygons.

- **Polygons:** Structure (GEOS-compatible fast path) or Arrange (CDT fallback)
- **Lines:** Self-intersection noding with snap-rounding fallback
- **Points:** NaN/Inf filtering, deduplication
- **Validation:** 18 OGC Simple Features predicates, Shewchuk adaptive-precision arithmetic

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
geo-repair = "0.11"
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

> **Experimental** — the two repair strategies and their auto-selection
> logic are still being tuned.  Heuristics, fallback thresholds, and
> algorithm details will change.

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

## I/O

GeoRepair provides format-agnostic dispatch and individual backends:

```rust
use geo_repair::{
    diagnose_file, load, load_bin, read_wkb, read_wkb_concat, repair_file, save, write_wkb,
    MakeValidConfig,
};

// Auto-detect format by extension (built-in: .wkb, .bin; gated: .shp, .wkt, .csv, .gml, .gpkg)
let geoms = load("input.wkb").unwrap();

// Concatenated multi-geometry WKB
let geoms = read_wkb_concat(&concat_buffer).unwrap();

// Validate each geometry (like GEOS isValidReason)
for result in diagnose_file("input.bin").unwrap() {
    println!("{}", result.reason());
}

// Repair + save (auto-detect format by extension)
repair_file("invalid.wkb", "fixed.wkb", &MakeValidConfig::default()).unwrap();

// Save geometry
save("output.wkb", &geoms[0]).unwrap();

// Low-level: WKB roundtrip
let wkb: Vec<u8> = write_wkb(&geom);
let geom = read_wkb(&wkb).unwrap();

// Load polygons from custom binary format
let polys = load_bin("dataset.bin").unwrap();
```

| Extension | Format | Backend |
|-----------|--------|---------|
| `.wkb` / `.wks` | WKB (LE/BE, EWKB SRID, Z/M variants) | Zero-dep built-in |
| `.bin` | Custom binary bulk polygon format | Zero-dep built-in |
| `.shp` | Shapefile | `io-shp` feature |
| `.wkt` | WKT text format | `io-wkt` feature |
| `.csv` | CSV with WKT geometry | `io-csv` feature |
| `.gml` | GML/XML | `io-gml` feature |
| `.gpkg` | GeoPackage (SQLite) | `io-gpkg` feature |

Optional backends behind feature flags: Shapefile (`io-shp`), WKT (`io-wkt`),
CSV (`io-csv`), GML (`io-gml`), GeoPackage (`io-gpkg`).

**SHP to .bin conversion:** `python scripts/convert_shp_to_bin.py input.shp output.bin`

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
| `io-wkt` | WKT text format backend | no |
| `io-csv` | CSV format backend | no |
| `io-gml` | GML/XML format backend | no |
| `io-gpkg` | GeoPackage format backend (not WASM) | no |
| `io-all` | All opt-in backends except gpkg | no |
| `io-all-native` | All opt-in backends including gpkg | no |
| `bench-geos` | GEOS comparison benchmarks | no |
| `bench-criterion` | Criterion benchmark harness | no |

## Python bindings

> **Experimental** — the Python API (function names, error handling, type
> signatures) is still settling.  Expect changes in naming and behaviour.

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

> **Experimental** — the C API is a work in progress.  Function names,
> type definitions, and memory ownership rules may change without notice.

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

### Portability

| Platform | Core | SIMD | I/O | Parallel | Python |
|----------|------|------|-----|----------|--------|
| x86_64 Windows/Linux/macOS | Yes | Yes (AVX2/AVX-512) | Yes | Yes | Yes |
| aarch64 macOS/Linux | Yes | Scalar | Yes | Yes | Yes |
| WASM32 | Yes | Scalar | In-memory only | No | No |
| no_std (embedded) | Yes | Scalar | No | No | No |

AVX2/AVX-512 requires `RUSTFLAGS="-C target-cpu=native"` at build time.
Falls back to scalar on CPUs without AVX2 or non-x86_64 targets.

## License

Apache-2.0

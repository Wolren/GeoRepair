<div align="center">

# GeoRepair

OGC geometry repair and validation for Rust. Passes the GEOS XML validation suite.

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.85+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)
[![Status](https://img.shields.io/badge/status-experimental-orange.svg)]()

</div>

> **This crate is experimental.** The API is actively evolving: expect
> breaking changes between 0.x releases. Core algorithms, I/O backends,
> and feature flags are all subject to change as we improve correctness
> and performance.

Detects and fixes invalid GIS geometries (self-intersections, unclosed
rings, degenerate shapes, NaN coordinates) using algorithms selected by
geometry type. Built-in I/O for WKB, WKT, and a custom binary batch
format with no extra dependencies.

The **Structure** strategy (default) mirrors GEOS ST_MakeValid: planar
graph extraction, face walking, winding-number assembly. The **Arrange**
strategy is a CDT-based fallback for complex topologies.

Performance on the 1,579,030-polygon production dataset (i5-12400F,
release, parallel batch, GEOS 3.14.1 conda-forge as reference):

| Dataset | GeoRepair | GEOS | vs GEOS |
|---------|----------:|-----:|:-------:|
| Validation (1.58M) | **3.0-3.2 s** | 3.6-3.9 s | **0.75-0.9x** |
| Invalid subset | 0 polys (2026-08-06) | 0 | - |
| Full dataset (1.58M polys) | 3.8-4.0 s | **3.3-3.5 s** | 1.1-1.2x |

## Performance

### Real-world dataset (1,579,030 polygons)

Structure batch on a production GIS dataset (813 features from a GeoPackage,
flattened to 1,579,030 polygon parts). i5-12400F (6C/12T), release LTO,
Rayon 12-thread batch; GEOS is conda-forge 3.14.1, serial per-call, run
concurrently via Rayon, geometries built via CoordSeq direct construction
(the one-time 1.5 s pre-build is excluded). Ratio is GEOS / GeoRepair:
**>1 means GeoRepair is faster** (same convention as the synthetic tables).

| Dataset | GeoRepair | GEOS | Ratio |
|---------|----------:|-----:|------:|
| Validation (1.58M) | **2.5 s** (1.6 µs/poly) | 3.5 s (2.2 µs/poly) | **1.4x** |
| Full pass (1.58M) | 4.0 s (2.6 µs/poly) | **3.5 s** (2.2 µs/poly) | 0.87x |

Validity agreement with GEOS: 100% (0/0 disagreements, 2026-08-07).

### Why large valid inputs are slower

Same asymptotics as GEOS (radix sweep, O(n log n)); the gap is constants.
GeoRepair validates at the eps-class (1e-12 x scale): the validator, the
gate, and the noder share one tolerance, so repair output is certifiable
by construction - GEOS's own `makeValid` can ship geometry its `isValid`
rejects. That contract is why the valid rows lose: tolerance predicates
cost more than GEOS's exact-zero ones, and the valid-polygon path makes
~5-6 passes over the coords vs GEOS's ~2 (a valid 5000-vertex polygon:
~148 µs serial vs GEOS ~78 µs; the parallel rows are bandwidth-bound, so
the throughput gap ~2.8x is wider than the serial ~1.9x). The eps-class
is stricter than GEOS only on borderline inputs (213 baselined XML
divergences, mostly the OGC winding contract); the 1.58M real-world
dataset agrees with GEOS 0/0 (winding-agnostic). Details:
`docs/BENCHMARKS.md`.

Lines carry one deliberate extra cost: the valid-or-empty contract.
`make_valid` never ships a non-simple line - GEOS passes non-simple lines
through unchanged (its `makeValid` is a no-op for line noding, verified
against GeometryFixer.cpp) - so every valid line pays an O(n) simplicity
check. The GEOS reference for the invalid-line rows is therefore
`UnaryUnion` (the operation GEOS users actually call to fix linework),
not `makeValid`. The check went 30-35x GEOS isSimple (rstar bulk_load) ->
~7x (radix sweep) -> ~2.4x (2026-08-07 lean pass); valid ls 500v: 4.3 ->
1.33 µs. Details: `docs/BENCHMARKS.md`.

### Synthetic benchmarks

Full tables and methodology: `docs/BENCHMARKS.md`. Representative rows (µs,
parallel batch; ratio = GEOS / GeoRepair, >1 means we win). Note: the
legacy "invalid star 100v" row is mislabelled - geosop isValid confirms
the star generator produces VALID geometry at every size (2026-08-07); it
measures a valid spiky polygon, not invalid repair. Genuine invalid-at-
scale coverage is the bowtie and spaghetti rows:

| Benchmark | GeoRepair | GEOS | Ratio |
|-----------|----------:|-----:|------:|
| invalid bowtie 4v | 0.87 | 18.9 | 22x |
| invalid bowtie 50v | 8.0 | 68.9 | 8.6x |
| invalid bowtie 100v | 29.4 | 135 | 4.6x |
| invalid bowtie 500v | 62.1 | 507 | 8.2x |
| spaghetti 500v | 4631 | 3001 | 0.65x |
| collapsed poly | 0.61 | 10.5 | 17x |
| overlap mp 50sh | 22.3 | 230 | 10x |
| dense grid 20x20=400 | 500 | 2114 | 4.2x |
| hole hier 50h | 22.2 | 27.7 | 1.25x |
| valid polygon 5000v | 25.7 | 9.3 | 0.36x |
| valid ls 500v | 1.33 | 0.56 | 0.42x |
| dense self ls 500v | 1.46 | 120 | 82x |
| lissajous 5000v | 597 | 5392 | 9.0x |
| collinear ov 500seg | 60 | 535 | 8.9x |
| star-comb 500sp | 75.6 | 688 | 9.1x |
| star-burst 500sp | 11.1 | 36413 | 3290x |
| spoke 500sp | 10.2 | 38841 | 3820x |
| mls 50x3v | 2.99 | 31.3 | 10x |

Pattern: GeoRepair wins on invalid repair and MultiPolygon unification:
dense grids 4.2-14x, overlaps 6.9-10x, bowties 4.6-22x at every size,
collinear-overlap families ~9x, and the line-repair family (zigzag,
spoke, star-burst, dense-self, lissajous) 5-3,800x - the star-burst and
spoke rows hit GEOS's quadratic worst cases. GEOS wins on large valid
polygons (0.33-0.90x) and valid ls >= 10v (0.42-0.58x) on the eps-class
constant (diagnosis above), on dense invalid rings (spaghetti 0.65x: the
noding leaves violations that force a snap-round + boolean fallback
chain; measured 2232 violations on a 500-edge walk), and two noise-level
rows (invalid star 100v 0.89x - a valid spiky polygon; collinear ls 500v
0.84x). Sub-µs rows fluctuate with Rayon dispatch overhead.

### Run benchmarks

```shell
# Real-world + synthetic with GEOS comparison (system GEOS, conda)
GEOS_LIB_DIR='D:\Miniconda\Library\lib' GEOS_INCLUDE_DIR='D:\Miniconda\Library\include' \
GEOS_VERSION=3.14.1 cargo bench --features bench-geos-system,arrange,structure,parallel,simd --bench real_world
GEOS_LIB_DIR='D:\Miniconda\Library\lib' GEOS_INCLUDE_DIR='D:\Miniconda\Library\include' \
GEOS_VERSION=3.14.1 cargo bench --features bench-geos-system,arrange,structure,parallel,simd --bench bench

# CI regression gate (no GEOS; curated subset vs benches/bench_baseline.json)
python scripts/bench_gate.py
```

Measurement rules that matter: always take the settled second run
(first-run-after-build is inflated ~18%); never trust a bench binary you
cannot trace to a source file. Full GEOS setup: `benches/AGENTS.md`.

## Limitations

1. **GEOS comparison is against conda-forge MSVC GEOS** (serial per-call,
   no LTO, no mimalloc) - a static LLVM-built GEOS would improve the GEOS
   side of every table.
2. **Validator strictness is deliberate:** eps-class predicates (1e-12 x
   scale tolerance, fast-FP-first with robust escalation). On the 1.58M
   real-world dataset it agrees with GEOS 0/0 (winding-agnostic); it
   diverges on borderline inputs (213 baselined XML cases, mostly the OGC
   winding contract - `docs/BENCHMARKS.md`). Repair ships only
   validator-clean geometry and degrades to an empty GeometryCollection
   otherwise.
3. **W/12 pool-saturation floor:** the parallel batch fills all 12 workers
   with giants; nested intra-poly rayon finds no idle threads in-batch
   (Amdahl-bound; standalone the parallel check measures 96 -> 53 ms).
4. **Giants (>4,096 ring edges) route to the boolean pipeline** - on a
   200k-edge giant single-pass noding costs 168 ms vs 36 ms for the
   boolean path (`SP_MAX_EDGES` in `src/core/mod.rs`).
5. **Mass-overlap repairs are the slowest synthetic class** (~0.57 ms/poly
   at dense grid 20x20) but stay ~195x faster than GEOS on the same shapes.
6. **Python bindings: `tests/test_python.py` covers the WKT surface**
   (18 tests); GeoJSON bindings were removed by design.
7. **`simd-portable` is nightly-only** (3 E0554 on stable, expected);
   hand-written AVX2 beyond the bbox scan measured slower than
   auto-vectorized scalar (point_in_ring 8.4x, is_ring_ccw 2.8x), and
   `-C target-cpu=native` regresses the full pass ~25%.
8. **`proj` requires native PROJ and is mutually exclusive with `io-gpkg`**
   (sqlite3 link conflict); `io-gpkg` is a default feature, so `proj` users
   build with `--no-default-features --features proj,...`.
9. **GEOS XML suite:** 937/937 dispatched cases pass; 3,629 overlay/relate
   cases are skipped (out of scope); 213 masked divergences (documented
   tolerance gates). The suite's WKT/WKB readers are our own - external
   `wkt`/`wkb` crates are disallowed because they measure slower.
10. **Sub-µs synthetic rows are noise** (Rayon dispatch overhead); the
    trustworthy metrics are the real-world batch numbers and the larger
    synthetic rows.

## Integration with the geo ecosystem

geo-repair is built on `geo` types and plugs into the georust ecosystem two
ways:

- **geo-traits sources** (`geo-traits` feature). The `interop` module runs
  validation and repair over `geo_traits::GeometryTrait` /
  `geo_traits::GeometryCollectionTrait`, the trait layer implemented by
  `geo`, geoarrow, geozero, and `wkb`. Any such source can be validated or
  repaired in one call (`interop::is_valid_geometry`,
  `interop::make_valid_geometry`, `interop::make_valid_geometries`, ...)
  without materializing `geo` types; results come back as `geo::Geometry<f64>`.

- **geo's `Validation` trait** (always available). `GeoRepairValidation(&geometry)`
  wraps any `&geo::Geometry<f64>` and exposes geo_repair's validator through
  geo's `Validation` trait (`.is_valid()`, `.check_validation()`,
  `.validation_errors()`) with geo's `Invalid*` error taxonomy. The orphan
  rule prevents implementing geo's trait for geo's own types, so the adapter
  is the bridge. Mapping is best-effort: geo_repair's stricter gates (32-ulp
  collinear, T-junction) surface through it, and classes geo does not model
  (ring closure, orientation, duplicates, ...) are omitted from the geo view.

```rust
use geo::algorithm::validation::Validation;
use geo_repair::GeoRepairValidation;

let adapter = GeoRepairValidation(&geometry);
assert!(!adapter.is_valid());
```

## Python bindings

The `geo-repair` PyPI package exposes the full validation and repair
surface over WKB bytes and WKT text, single geometry and batch
(including a rayon-backed parallel batch). abi3 wheels (`cp38-abi3`)
serve Python 3.8+; typing stubs are shipped in the wheel.

```bash
pip install geo-repair
```

```python
import geo_repair

fixed = geo_repair.repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")
assert geo_repair.is_valid_wkt(fixed)

was_valid, errors, fixed_wkb = geo_repair.validate_and_fix_wkb(wkb_bytes)
results = geo_repair.par_repair_wkb_batch(list_of_wkb_bytes)
```

Every function exists for WKB and WKT (`repair_*`, `repair_*_batch`,
`par_repair_*_batch`, `repair_validate_*`, `is_valid_*`, `validate_*`,
`validate_and_fix_*`, plus batch forms), with `method`
(auto/arrange/structure) and `keep_collapsed` parameters. A QGIS
Processing script (`qgis/qgis_geo_repair.py`) streams features through
the WKB batch API. Full API reference: `docs/BINDINGS.md`.

## C API

The `ffi` feature exposes a panic-safe C API over WKB and WKT, single
geometries and parallel batches:

```bash
cargo build --release --features ffi
# -> target/release/geo_repair.{dll,so,dylib} + libgeo_repair.a + include/geo_repair.h
```

```c
#include "geo_repair.h"

GeoRepairResult r = geo_repair_make_valid(bowtie_wkb, bowtie_wkb_len);
if (r.success) { /* r.wkb_data / r.wkb_len = fixed WKB */ }
geo_repair_free_result(&r);
```

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
| `parallel` | Rayon parallel processing (non-WASM) | yes |
| `simd` | Runtime-dispatched AVX2 bbox kernel (the one SIMD kernel that beat auto-vectorized scalar, 4.5x); all other kernels auto-vectorized scalar | yes |
| `validate` | OGC validation predicates | yes |
| `mimalloc` | Use mimalloc global allocator | yes |
| `std` | Standard library + file I/O. Disable for no_std builds. | yes |
| `simd-portable` | Portable SIMD via `core::simd` (nightly only) | no |
| `memmap` | Memory-mapped binary file loading | no |
| `wasm` | WASM browser fetch (synchronous XHR) | no |
| `proj` | CRS transformation (placeholder) | no |
| `serde` | Geometry serde support (`geo/serde`) | no |
| `ffi` | C-compatible FFI bindings | no |
| `geo-traits` | Interop surface over `geo_traits::GeometryTrait` / `GeometryCollectionTrait` (geo, geoarrow, geozero, wkb sources) | no |
| `python` | Python bindings via PyO3 | no |
| `io-shp` | Shapefile format backend | no |
| `io-wkt` | No-op (WKT is built-in, kept for CI compatibility) | no |
| `io-csv` | CSV format backend | no |
| `io-gml` | GML/XML format backend | no |
| `io-gpkg` | GeoPackage format backend (default; gated out on wasm32) | yes |
| `io-all` | All opt-in backends except gpkg | no |
| `io-all-native` | All opt-in backends including gpkg | no |
| `bench-geos` | GEOS comparison benchmarks (build from source, MSVC, no LTO) | no |
| `bench-geos-system` | GEOS comparison benchmarks (link against system GEOS, conda-forge MSVC) | no |

## License

Apache-2.0

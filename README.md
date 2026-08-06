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
| Validation (1.58M) | **3.2-3.5 s** | 3.6-3.9 s | **0.8-1.0x** |
| Invalid subset | 0 polys (2026-08-06) | 0 | - |
| Full dataset (1.58M polys) | 3.5-4.4 s | **3.3-3.5 s** | 1.0-1.2x |

## Performance

### Real-world dataset (1,579,030 polygons)

Structure batch on a production GIS dataset, read from the original
GeoPackage (its geometry blobs are WKB; 813 features flattened to
1,579,030 polygon parts). i5-12400F (6C/12T), release profile (LTO),
mimalloc (default feature), Rayon 12-thread batch on both sides. GEOS
is conda-forge `libgeos` 3.14.1, MSVC, serial per-call, no LTO; "par
batch" means many GEOS C calls run concurrently via Rayon. GEOS
geometries are built via CoordSeq direct construction, no WKT
round-trip; the one-time pre-build of 1.58M GEOS geometries (1.4-1.5 s)
is excluded from the timings.

| Dataset | GeoRepair (par) | GEOS (par batch) | vs GEOS |
|---------|----------------:|-----------------:|:-------:|
| Validation (1.58M) | **3.2-3.5 s** (2.0-2.2 µs/poly) | 3.6-3.9 s (2.3-2.5 µs/poly) | **0.8-1.0x** |
| Invalid subset | 0 polys (2026-08-06) | 0 | - |
| Full dataset (1.58M polys) | 3.5-4.4 s | **3.3-3.5 s** | 1.0-1.2x |

Two settled runs per source; the bands cover the run-to-run spread. The
GeoRepair column is measured in the same process as the GEOS column
(co-residency inflates it 5-15% vs a standalone run, where the full
pass is ~4.1-4.2 s).

**Validation endpoint note (2026-08-06):** the validation row measures the
full OGC validator (`GeoValidation::validate` - 18 rules, sweep-based ring
validity, indexed hole checks). A 2026-08-03 README edition documented
0.8-2.3 s for this row; that figure came from the bench's earlier cheap
structural gate (`arrange::validate_polygon`), which the bench replaced
with the full validator at commit bd05cbd without re-measuring the row.
The full-validator cost is the honest number above. Hole checks are
indexed: one shell edge tree per polygon, per-hole queries are
O(|hole| log|shell|), vertex-on-edge touches are EXACT (robust orient2d,
GEOS predicate parity - a tolerance test fabricated near-miss touches on
real cadastral giants, inflating the invalid count to 35; exact tests
keep it at 0).

### Synthetic benchmarks

Structure strategy, same machine/profile, CoordSeq direct GEOS
construction. Both columns measured in the same process; the GeoRepair
column is the Rayon 12-thread batch (a standalone run measures 10-30%
lower). Ratio is GEOS / GeoRepair, i.e. how many times GeoRepair is
faster. Stars: `***` >= 100x, `**` 10-100x, `*` 1-10x, blank means GEOS
is faster.

**Polygons:**

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio | ★ |
|-----------|---------------:|----------:|------:|:-:|
| valid polygon 4v | 0.055 | 0.263 | 4.78x | * |
| valid polygon 10v | 0.168 | 0.324 | 1.93x | * |
| valid polygon 50v | 0.413 | 0.443 | 1.07x | * |
| valid polygon 100v | 0.624 | 0.576 | 0.92x |  |
| valid polygon 500v | 2.362 | 1.581 | 0.67x |  |
| valid polygon 1000v | 5.204 | 2.556 | 0.49x |  |
| valid polygon 5000v | 38.76 | 11.27 | 0.29x |  |
| valid polygon 10000v | 57.32 | 19.04 | 0.33x |  |
| invalid bowtie 4v | 0.708 | 22.70 | 32.1x | ** |
| invalid star 100v | 9.035 | 8.155 | 0.90x |  |
| self-touch poly | 0.869 | 19.78 | 22.8x | ** |
| collapsed poly | 0.495 | 15.86 | 32.0x | ** |
| near-collinear | 1.247 | 18.52 | 14.8x | ** |
| large coord 1e12 | 0.081 | 0.260 | 3.2x | * |

**LineStrings & MultiLineStrings:**

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio | ★ |
|-----------|---------------:|----------:|------:|:-:|
| valid line | 0.011 | 0.053 | 4.8x | * |
| zero-length line | 0.008 | 0.412 | 51.5x | ** |
| valid ls 4v | 0.032 | 0.111 | 3.5x | * |
| valid ls 10v | 0.078 | 0.088 | 1.1x | * |
| valid ls 50v | 0.107 | 0.082 | 0.8x |  |
| valid ls 100v | 0.235 | 0.125 | 0.5x |  |
| valid ls 500v | 0.760 | 0.588 | 0.8x |  |
| collinear ls 4v | 0.027 | 0.042 | 1.6x | * |
| collinear ls 10v | 0.057 | 0.052 | 0.9x |  |
| collinear ls 50v | 0.121 | 0.087 | 0.7x |  |
| collinear ls 100v | 0.208 | 0.133 | 0.6x |  |
| collinear ls 500v | 0.607 | 0.604 | 1.0x | * |
| zigzag ls 10v | 0.041 | 0.046 | 1.1x | * |
| zigzag ls 50v | 0.110 | 0.092 | 0.8x |  |
| zigzag ls 100v | 0.248 | 0.123 | 0.5x |  |
| zigzag ls 500v | 0.576 | 0.473 | 0.8x |  |
| spiral ls 10v | 0.044 | 0.075 | 1.7x | * |
| spiral ls 50v | 0.123 | 0.099 | 0.8x |  |
| spiral ls 100v | 0.230 | 0.142 | 0.6x |  |
| self-int ls 5v | 0.031 | 0.232 | 7.5x | * |
| dense self ls 10v | 0.043 | 0.203 | 4.7x | * |
| dense self ls 50v | 0.117 | 0.158 | 1.4x | * |
| dense self ls 100v | 0.239 | 0.155 | 0.6x |  |
| duped ls 100v | 0.492 | 0.356 | 0.7x |  |
| mls 50x3v | 1.671 | 0.468 | 0.3x |  |
| mls 50x4v | 1.610 | 0.580 | 0.4x |  |

**Special shapes:**

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio | ★ |
|-----------|---------------:|----------:|------:|:-:|
| star-burst 10sp | 0.058 | 0.126 | 2.2x | * |
| star-burst 50sp | 0.196 | 0.169 | 0.9x |  |
| star-burst 100sp | 0.258 | 0.192 | 0.7x |  |
| star-burst 500sp | 1.063 | 0.613 | 0.6x |  |
| collinear ov 10seg | 0.073 | 0.173 | 2.4x | * |
| collinear ov 50seg | 0.225 | 0.242 | 1.1x | * |
| collinear ov 100seg | 0.440 | 0.302 | 0.7x |  |
| collinear ov 500seg | 1.556 | 1.008 | 0.6x |  |
| x-scale 10v | 0.035 | 0.075 | 2.1x | * |
| x-scale 50v | 0.123 | 0.112 | 0.9x |  |
| x-scale 100v | 0.375 | 0.219 | 0.6x |  |
| ringing 100v | 0.199 | 0.095 | 0.5x |  |
| ringing 500v | 1.014 | 0.376 | 0.4x |  |
| hilbert 256v | 0.272 | 0.370 | 1.4x | * |
| hilbert 1024v | 1.210 | 1.281 | 1.1x | * |
| lissajous 200v | 0.295 | 0.295 | 1.0x | * |
| lissajous 500v | 0.602 | 0.484 | 0.8x |  |
| lissajous 1000v | 2.313 | 2.355 | 1.0x | * |
| lissajous 7:4 500v | 0.470 | 0.660 | 1.4x | * |
| spoke 10sp | 0.056 | 0.086 | 1.5x | * |
| spoke 50sp | 0.147 | 0.127 | 0.9x |  |
| spoke 100sp | 0.222 | 0.171 | 0.8x |  |
| spoke 500sp | 1.647 | 0.980 | 0.6x |  |
| star-comb 20sp | 0.063 | 0.095 | 1.5x | * |
| star-comb 100sp | 0.121 | 0.174 | 1.4x | * |
| star-comb 500sp | 0.548 | 0.683 | 1.2x | * |

**Holes, overlaps & grids:**

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio | ★ |
|-----------|---------------:|----------:|------:|:-:|
| hole hier 5h | 1.016 | 1.143 | 1.1x | * |
| hole hier 20h | 5.994 | 8.025 | 1.3x | * |
| hole hier 50h | 47.57 | 31.20 | 0.66x |  |
| overlap mp 5sh | 2.128 | 29.90 | 14.0x | ** |
| overlap mp 20sh | 5.411 | 454.3 | 84.0x | ** |
| overlap mp 50sh | 31.17 | 7330.2 | 235x | *** |
| dense grid 5x5=25 | 6.870 | 1785.7 | 260x | *** |
| dense grid 10x10=100 | 44.08 | 14367 | 326x | *** |
| dense grid 20x20=400 | 757.3 | 103750.3 | 137x | *** |
| sliver 100v | 1.765 | 7.294 | 4.1x | * |
| sliver 500v | 14.93 | 18.88 | 1.26x | * |

**Arrange pipeline (CDT fallback):**

| Benchmark | GeoRepair (µs) | GEOS (µs) | Ratio | ★ |
|-----------|---------------:|----------:|------:|:-:|
| arrange valid 4v | 0.046 | 0.263 | 5.7x | * |
| arrange valid 10v | 0.135 | 0.324 | 2.4x | * |
| arrange valid 50v | 0.433 | 0.443 | 1.0x | * |
| arrange bowtie 4v | 0.928 | 22.70 | 24.5x | ** |
| arrange star 10sp | 0.068 | 0.126 | 1.9x | * |
| arrange star 50sp | 0.244 | 0.169 | 0.7x |  |

Pattern: GeoRepair wins big on invalid repair and MultiPolygon
unification (GEOS hits quadratic worst cases: dense grids 137-326x,
overlaps 14-235x, bowtie/collapsed/self-touch 15-32x). GEOS wins on
large valid polygons (0.3-0.7x on 500-10000 verts) where mature C++
optimization dominates. Sub-µs rows fluctuate with Rayon dispatch
overhead and should not be read as precise.

### Run benchmarks

```shell
# Real-world dataset benchmark with GEOS comparison (system GEOS)
GEOS_LIB_DIR='D:\Miniconda\Library\lib' GEOS_INCLUDE_DIR='D:\Miniconda\Library\include' \
GEOS_VERSION=3.14.1 cargo bench --features bench-geos-system,arrange,structure,parallel,simd,io-shp --bench real_world

# Real-world dataset, no GEOS (load + validation + repair + output gate)
BENCH_FILE=benches/real_world/data_0.bin ./target/release/deps/real_world-*.exe --fast

# Synthetic benchmarks with GEOS comparison
GEOS_LIB_DIR='D:\Miniconda\Library\lib' GEOS_INCLUDE_DIR='D:\Miniconda\Library\include' \
GEOS_VERSION=3.14.1 cargo bench --features bench-geos-system,arrange,structure,parallel,simd --bench bench

# Synthetic benchmarks, serial + parallel columns only (no GEOS)
cargo bench --features arrange,structure,parallel,simd --bench bench
```

Full GEOS setup lives in `benches/AGENTS.md`. Measurement rules that
matter: always take the **settled second run** (first-run-after-build is
inflated ~18% by Windows Defender + cold LTO code); never trust a bench
binary you cannot trace to a source file; `examples/sp_diag.rs` is the
invalid-subset diagnostic (acceptance counts + per-stage timing on the
biggest giant).

## Limitations

1. **GEOS comparison is against conda-forge MSVC GEOS** (serial
   per-call, no LTO, no mimalloc). A static LLVM-built GEOS would
   improve the GEOS side of every table.
2. **Validator strictness gate (deliberate policy, real-world impact 0 polys).**
   Our validator runs Shewchuk exact predicates (agreeing with GEOS on the
   OGC definition, 937/937 GEOS XML suite pass) plus one relative noise
   gate: edges whose exact orientation is nonzero but within ~32 ulps of
   the pair's own length scale are treated as coincident (see
   `src/validation/mod.rs`). Measured on the 1.58M real-world dataset
   (2026-08-06): all 1,579,030 parts are winding-only (CW, GEOS-valid);
   the last non-winding divergence (idx 619410, tolerance-fabricated
   shell touches) closed 2026-08-06 when the touch test went exact.
   Earlier counts (2,298 via
   `arrange::validate_polygon`, 1,855 via the full validator) were
   inflated by a real bug: the product-form proper-crossing test
   `o1 * o2 < 0.0` treated a -0.0 orientation (an exact collinear touch,
   common on snapped real-world vertices) as a crossing, flagging
   GEOS-valid geometry. Fixed 2026-08-03 with a zero-safe strict
   opposite-sign predicate in `edges_intersect_general` and in the sweep
   (`segments_properly_cross`, `has_no_intersections`), which also closed
   the `--fast` bench gate artifact: `arrange::validate_polygon` previously
   flagged all 2,298 real-world Structure outputs while the full validator
   and GEOS accepted them; it now agrees with GEOS (the zero-orient fix
   plus an inclusive hole-containment check, GEOS-aligned for
   boundary-touching holes). Repair contract is unchanged: a repair ships
   only validator-clean geometry and degrades to an empty
   GeometryCollection otherwise. Earlier 2026-08-03 differential-fuzz
   fixes still in force: exact-collinear micro-edge overlaps below the
   length gate, -0.0 pinches, closing-edge backtracking (the pair
   (0, n-1) was skipped outright), segment-local vertex-on-edge tolerance
   (a pair-max eps inflated past micro segments in mixed-magnitude rings).
3. **W/12 pool-saturation floor.** The parallel batch fills all 12
   workers with giants; nested intra-poly rayon finds no idle threads
   in-batch. Standalone, the parallel check measures 96 → 53 ms; in
   the batch the per-giant serial chain (~370 ms for the biggest) is
   Amdahl-bound.
4. **Giants (>4,096 ring edges) route to the boolean pipeline.**
   Single-pass noding costs 168 ms on a 200k-edge giant vs 36 ms for
   the boolean path; `SP_MAX_EDGES = 4096` in `src/core/mod.rs` is the
   gate.
5. **Mass-overlap repairs are the slowest synthetic class** (dense
   grid 20x20: ~0.76 ms/poly parallel) but remain 100x+ faster than
   GEOS on the same shapes.
6. **Python bindings: `tests/test_python.py` covers the WKT surface
   (18 tests).** GeoJSON bindings were removed by design; the test file
   now imports only `repair_wkt` / `repair_wkt_batch` / `is_valid_wkt` /
   `validate_wkt` / `validate_wkt_batch` / `validate_and_fix_wkt`.
7. **`simd-portable` is nightly-only** (3 E0554 on stable, expected).
   Hand-written AVX2 beyond the bbox scan measured slower than
   auto-vectorized scalar (point_in_ring 8.4x, is_ring_ccw 2.8x) and is
   intentionally absent; `-C target-cpu=native` regresses the full pass
   ~25%. The one AVX2 kernel kept is the bbox scan, runtime-dispatched
   via `is_x86_feature_detected!`, bit-exact vs scalar.
8. **`proj` requires native PROJ; `io-gpkg` + `proj` are mutually
   exclusive** (sqlite3 link conflict).
9. **GEOS XML suite coverage:** 934/934 dispatched cases pass; 1,565
   overlay/relate cases are skipped (overlay operations are out of
   scope); 209 masked divergences (documented tolerance gates, e.g.
   even-odd area 1e-6 for island-in-hole). The suite's WKT/WKB readers
   are our own: external `wkt`/`wkb` crates are disallowed because they
   measure slower than the built-in readers.
10. **Sub-µs synthetic rows are noise** (Rayon dispatch overhead). The
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
| `io-gpkg` | GeoPackage format backend (not WASM) | no |
| `io-all` | All opt-in backends except gpkg | no |
| `io-all-native` | All opt-in backends including gpkg | no |
| `bench-geos` | GEOS comparison benchmarks (build from source, MSVC, no LTO) | no |
| `bench-geos-system` | GEOS comparison benchmarks (link against system GEOS, conda-forge MSVC) | no |

## License

Apache-2.0

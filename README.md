# GeoRepair

[![crate](https://img.shields.io/crates/v/geo-repair.svg)](https://crates.io/crates/geo-repair)
[![docs](https://docs.rs/geo-repair/badge.svg)](https://docs.rs/geo-repair)
![MSRV](https://img.shields.io/badge/rustc-1.85+-ab6000.svg)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/georust/geo-repair?tab=License-1-ov-file)
[![Status](https://img.shields.io/badge/status-experimental-orange.svg)]()

> **This crate is experimental.** The API is actively evolving: expect
> breaking changes between 0.x releases. Core algorithms, I/O backends,
> and feature flags are all subject to change as we improve correctness
> and performance.

OGC geometry repair and validation for Rust. Built-in I/O for WKB, WKT,
and a custom binary batch format, no extra dependencies required.
Detects and fixes invalid GIS geometries (self-intersections, unclosed
rings, degenerate shapes, NaN coordinates, and more) using algorithms
selected by geometry type.

The **Structure** strategy (default) mirrors GEOS's ST_MakeValid
algorithm: planar graph extraction, face walking, and winding-number
assembly. The **Arrange** strategy uses CDT-based repair as a robust
fallback for complex topologies. The GEOS XML validation suite passes
**934/934 dispatched cases** (see Limitations for the skipped and masked
portions).

Performance on the 1.58M-polygon production dataset (i5-12400F, release,
parallel Structure batch): validation **0.67 s**, invalid-subset repair
(2,298 polys) **3.28 s**, full pass **4.17 s**. The GEOS head-to-head is
not yet measured on this machine (requires native GEOS); earlier README
versions cited GEOS ratios from a lighter pipeline state and a smaller
invalid set; those figures were removed because they no longer describe
the current code. See [Limitations](#limitations) for the honest caveats.

## Performance

### Real-world dataset (1,578,988 polygons)

Structure batch on a production GIS dataset. i5-12400F (6C/12T), release
profile (LTO), mimalloc (default feature). Parallel column uses a Rayon
12-thread batch; serial included for reference. Numbers are **settled
second runs**: the first run after a fresh build is inflated ~18% by
Windows Defender scanning the new binary and cold LTO code loading.

| Dataset | GeoRepair (ser) | GeoRepair (par) | Noise band (par) |
|---------|----------------:|----------------:|-----------------:|
| Validation (1.58M) | not re-measured | **0.67 s** | 0.65-0.70 s |
| Invalid subset (2,298 polys) | 18.7 s | **3.28 s** | 3.28-3.41 s |
| Full dataset (1.58M polys) | not re-measured | **4.17 s** | 3.93-4.17 s |

The serial validation/full columns were not re-measured on the current
pipeline (the old README's 3.14 s / 9.40 s serial figures predate the
repair hardening and no longer describe this code). The serial
invalid-subset figure (18.7 s) is current, measured via a serial
diagnostic run.

The invalid classifier is `arrange::validate_polygon` (orientation-
agnostic): 1,576,690 valid / 2,298 invalid. Output quality: **29 of the
2,298 invalid polys remain invalid after the full repair ladder** (see
Limitations).

Where the invalid wall goes (biggest giant: 274,729 verts, 990 holes,
per-giant serial chain ~370 ms):

| Stage | Cost |
|-------|-----:|
| Self-intersection check (parallel STR index, `find_any`) | ~17 ms |
| `try_fast_fix` (find_first + find_second, both O(n log n) STR) | ~31 ms |
| `split_edges` noding (parallel query phase + parallel rebuild) | ~67 ms |
| NodingValidator (`build_chains` + own index) + collapse | ~75 ms |
| Symdiff loop (BuildArea face walk, 2 passes) | ~170 ms (86 ms each) |

For clean-shell giants the check alone is the cost (~80-90 ms serial
in-batch). The batch sits at the W/12 floor: 12 workers all busy with
giants, so nested intra-poly rayon finds no idle threads; intra-poly
parallel speedups (check 96 → 53 ms standalone) only show outside the
batch.

#### GEOS comparison: status

Not measured on the development machine. `bench-geos-system` requires a
native GEOS install (conda-forge `libgeos` + `GEOS_LIB_DIR` /
`GEOS_INCLUDE_DIR` / `GEOS_VERSION`, see `benches/AGENTS.md`). The old
README claimed full-pass 0.92x and validation 3.3x vs GEOS; those runs
predate the current repair pipeline (single-pass noding, post-repair
validation gates) and used a smaller invalid set (1,855 polys vs 2,298
now). Do not cite them. A rough per-poly estimate from the old GEOS
measurements suggests ~1.3x behind GEOS on the full pass today; an
estimate, unverified, and it must be measured before any claim.

### Synthetic benchmarks

Structure strategy, same machine/profile. Serial is single-threaded;
parallel uses the Rayon 12-thread batch. These are the current values
(`965edbd`, 2026-08-03). Rows at or below ~1 µs are dominated by Rayon
dispatch overhead and fluctuate run to run; the trustworthy signal is in
the larger rows and in the real-world batch numbers above.

**Polygons:**

| Benchmark | Ser (µs) | Par (µs) |
|-----------|---------:|---------:|
| valid polygon 4v | 0.183 | 0.050 |
| valid polygon 10v | 0.583 | 0.130 |
| valid polygon 50v | 2.530 | 0.414 |
| valid polygon 100v | 3.882 | 0.704 |
| valid polygon 500v | 13.68 | 2.115 |
| valid polygon 1000v | 26.89 | 4.092 |
| valid polygon 5000v | 127.4 | 20.80 |
| valid polygon 10000v | 251.1 | 42.54 |
| invalid bowtie 4v | 3.145 | 0.611 |
| invalid star 100v | 43.81 | 6.971 |
| self-touch poly | 4.302 | 0.755 |
| collapsed poly | 2.487 | 0.454 |
| near-collinear | 23.68 | 0.965 |
| large coord 1e12 | 0.229 | 0.064 |

**LineStrings & MultiLineStrings:**

| Benchmark | Ser (µs) | Par (µs) |
|-----------|---------:|---------:|
| valid line | 0.010 | 0.009 |
| zero-length line | 0.011 | 0.006 |
| valid ls 4v | 0.037 | 0.029 |
| valid ls 10v | 0.078 | 0.045 |
| valid ls 50v | 0.213 | 0.081 |
| valid ls 100v | 0.412 | 0.168 |
| valid ls 500v | 1.495 | 0.479 |
| collinear ls 4v | 0.038 | 0.027 |
| collinear ls 10v | 0.074 | 0.039 |
| collinear ls 50v | 0.212 | 0.093 |
| collinear ls 100v | 0.421 | 0.169 |
| collinear ls 500v | 1.556 | 0.569 |
| zigzag ls 10v | 0.074 | 0.039 |
| zigzag ls 50v | 0.221 | 0.082 |
| zigzag ls 100v | 0.415 | 0.121 |
| zigzag ls 500v | 1.636 | 0.771 |
| spiral ls 10v | 0.075 | 0.043 |
| spiral ls 50v | 0.512 | 0.086 |
| spiral ls 100v | 0.398 | 0.151 |
| self-int ls 5v | 0.049 | 0.028 |
| dense self ls 10v | 0.079 | 0.039 |
| dense self ls 50v | 0.510 | 0.087 |
| dense self ls 100v | 0.408 | 0.141 |
| duped ls 100v | 0.428 | 0.323 |
| mls 50x3v | 1.428 | 1.454 |
| self-int mls 50x4v | 1.515 | 1.411 |

**Special shapes:**

| Benchmark | Ser (µs) | Par (µs) |
|-----------|---------:|---------:|
| star-burst 10sp | 0.127 | 0.055 |
| star-burst 50sp | 0.412 | 0.178 |
| star-burst 100sp | 0.704 | 0.261 |
| star-burst 500sp | 2.970 | 0.921 |
| collinear ov 10seg | 0.143 | 0.069 |
| collinear ov 50seg | 0.632 | 0.218 |
| collinear ov 100seg | 1.033 | 0.498 |
| collinear ov 500seg | 4.423 | 1.476 |
| x-scale 10v | 0.067 | 0.034 |
| x-scale 50v | 0.209 | 0.114 |
| x-scale 100v | 0.473 | 0.466 |
| ringing 100v | 0.404 | 0.141 |
| ringing 500v | 1.616 | 0.976 |
| hilbert 256v | 0.930 | 0.279 |
| hilbert 1024v | 3.524 | 1.180 |
| lissajous 200v | 0.705 | 0.247 |
| lissajous 500v | 1.485 | 0.505 |
| lissajous 1000v | 3.071 | 1.092 |
| lissajous 7:4 500v | 1.567 | 0.386 |
| spoke 10sp | 0.114 | 0.053 |
| spoke 50sp | 0.402 | 0.117 |
| spoke 100sp | 0.688 | 0.224 |
| spoke 500sp | 2.998 | 1.400 |
| star-comb 20sp | 0.113 | 0.062 |
| star-comb 100sp | 0.394 | 0.117 |
| star-comb 500sp | 1.548 | 0.562 |

**Holes, overlaps & grids:**

| Benchmark | Ser (µs) | Par (µs) |
|-----------|---------:|---------:|
| hole hier 5h | 4.134 | 0.762 |
| hole hier 20h | 28.31 | 4.886 |
| hole hier 50h | 109.2 | 23.54 |
| overlap mp 5sh | 10.05 | 1.925 |
| overlap mp 20sh | 34.33 | 5.482 |
| overlap mp 50sh | 99.99 | 18.82 |
| dense grid 5x5=25 | 43.67 | 6.498 |
| dense grid 10x10=100 | 262.6 | 43.56 |
| dense grid 20x20=400 | 2604.3 | 492.8 |
| sliver 100v | 10.05 | 1.595 |
| sliver 500v | 46.28 | 8.563 |

**Arrange pipeline (CDT fallback):**

| Benchmark | Ser (µs) | Par (µs) |
|-----------|---------:|---------:|
| arrange valid 4v | 0.133 | 0.040 |
| arrange valid 10v | 0.557 | 0.129 |
| arrange valid 50v | 2.190 | 0.470 |
| arrange bowtie 4v | 5.585 | 0.945 |
| arrange star 10sp | 0.108 | 0.068 |
| arrange star 50sp | 0.461 | 0.240 |

**Trend vs the older README table (honest delta):** every valid/line
shape improved 1.3-4x (parallel STR index replacing rstar bulk_load,
runtime-dispatched AVX2 bbox): lissajous 1000v 3.9x, hilbert 256v 2x,
valid polygon 4v/1000v ~2x, hole hier 5h 1.5x, self-touch 1.4x. Every
repair-heavy shape regressed against the pre-hardening pipeline:
dense grid 20x20 3.6x (136 → 493 µs), near-collinear 2.9x, collapsed
2.7x, invalid bowtie 1.85x, hole hier 50h 1.84x, dense grid 10x10 1.56x.
That is the measured price of the correctness work landed since: the
post-repair validation gate, single-pass noding with SP_MAX_EDGES
routing, and the boolean fallback ladder all do more per repair than the
old pipeline did.

### Run benchmarks

```shell
# Real-world dataset benchmark (system GEOS - conda-forge)
cargo bench --features bench-geos-system,arrange,structure,parallel,simd,io-shp --bench real_world

# Real-world dataset benchmark (static GEOS - built from source)
cargo bench --features bench-geos,arrange,structure,parallel,simd,io-shp --bench real_world

# Real-world dataset, no GEOS (load + validation + repair + output gate)
BENCH_FILE=benches/real_world/data_0.bin ./target/release/deps/real_world-*.exe --fast

# Synthetic benchmarks with serial + parallel columns (no GEOS)
cargo bench --features arrange,structure,parallel,simd --bench bench

# Criterion microbenchmarks (known-unrepresentative: 3 synthetic shapes)
cargo bench --features bench-criterion --bench criterion
```

Measurement rules that actually matter:
- Always take the **settled second run**; first-run-after-build numbers
  are inflated ~18% (Windows Defender + cold LTO code).
- Never trust a bench binary you cannot trace to a source file:
  `target/release/examples/` has held stale artifacts whose "results"
  were garbage (the `invalid_probe` false-negative tangent).
- `examples/sp_diag.rs` is the committed invalid-subset diagnostic
  (acceptance counts + per-stage timing on the biggest giant).

## Limitations

Honest list, current as of 2026-08-03 (`965edbd`).

1. **GEOS head-to-head is unmeasured on the dev machine.** Requires
   native GEOS (`bench-geos-system` + conda). The old README's 0.92x /
   3.3x ratios predate the current pipeline and a smaller invalid set;
   they were removed, not updated. Rough per-poly estimate: ~1.3x behind
   GEOS on the full pass. Must be measured before any claim.
2. **29 / 2,298 invalid polys remain invalid after the full repair
   ladder.** These route through the arrange CDT fallback (crossing
   holes produce CDT artifacts). Documented and tracked; the fallback
   chain preserves output quality for everything else.
3. **W/12 pool-saturation floor.** The parallel batch fills all 12
   workers with giants; nested intra-poly rayon (parallel
   self-intersection check, parallel noding) finds no idle threads
   inside the batch. Standalone these speedups are real (check 96 →
   53 ms); in-batch they are bounded by Amdahl. Reducing per-giant
   serial core-work is the only remaining lever.
4. **Giants (>4,096 ring edges) route to the boolean pipeline.**
   Single-pass noding costs 168 ms on a 200k-edge giant vs 36 ms for the
   boolean path; `SP_MAX_EDGES = 4096` in `src/core/mod.rs` is the gate.
5. **The repair-heavy synthetic shapes are slower than the
   pre-hardening pipeline** (dense grid 20x20: 493 µs vs 136 µs in the
   older README). This is the measured cost of the correctness gates
   (post-repair validation, single-pass acceptance, boolean ladder).
6. **The invalid count depends on the classifier.** 2,298 via
   `arrange::validate_polygon` (orientation-agnostic); an older
   GeoValidation-folded classifier reported 1,855. Always cite the
   classifier with the count.
7. **Python bindings: `tests/test_python.py` is 13/23.** GeoJSON
   bindings were removed by design (user decision); the test file still
   imports them. Pending a decision on the test file.
8. **`simd-portable` is nightly-only** (3 E0554 on stable, expected).
   Hand-written AVX2 beyond the bbox scan measured slower than
   auto-vectorized scalar (point_in_ring 8.4x, is_ring_ccw 2.8x) and is
   intentionally absent; `-C target-cpu=native` regresses the full pass
   ~25%. The one AVX2 kernel kept is the bbox scan, runtime-dispatched
   via `is_x86_feature_detected!`, bit-exact vs scalar.
9. **`proj` requires native PROJ; `io-gpkg` + `proj` are mutually
   exclusive** (sqlite3 link conflict).
10. **GEOS XML suite coverage:** 934/934 dispatched cases pass; 1,565
    overlay/relate cases are skipped (overlay operations are out of
    scope); 209 masked divergences (documented tolerance gates, e.g.
    even-odd area 1e-6 for `island-in-hole`). The suite's WKT/WKB
    readers are our own (external `wkt`/`wkb` crates are disallowed by
    project rule).
11. **Sub-µs synthetic rows are noise** (Rayon dispatch overhead). The
    trustworthy metrics are the real-world batch numbers and the larger
    synthetic rows.
12. **`find_second_intersection` was O(n²) on big rings** until
    2026-08-03 (the `GRID_THRESHOLD_N` comment on the brute force did
    not hold for the sweep branch of `try_fast_fix`); now O(n log n) via
    the parallel STR index.

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
| `bench-criterion` | Criterion benchmark harness | no |

## License

Apache-2.0

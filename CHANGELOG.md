# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.14.4] - 2026-08-09

### Changed

- Valid-input fast path: the plausibility gate now records each ring's
  extremal index, so OGC winding is an O(1) orient + conditional reversal
  and `strip_degenerate` + the NaN tail are skipped for certified
  results. Valid polygon 5000v: 27.2 to 19.0-19.3 us (1.74x vs GEOS).
- Pinch-table scratch buffer moved to a thread-local (no 256-512 KB
  alloc+memset per polygon on the fast path).
- Synthetic bench: GEOS reference now routes by geometry class (lines to
  UnaryUnion, polygons/MultiPolygons to makeValid). The label-prefix
  routing had silently swapped the MP rows to UnaryUnion (~46x cheaper
  on overlapping shells), collapsing dense-grid ratios to 4-16x; they
  are back to 190-281x.
- Synthetic bench: full size ladders (91 to 133 rows). Every family now
  reaches at least 1000 vertices (or the equivalent shell/part/hole
  count) so size-scaling regressions are measurable; arrange ladder
  extended to 100/500/1000v.
- README: internal-monologue sections removed; the full 133-row
  synthetic table is restored and gated in CI
  (`scripts/readme_bench_table.py --check` fails when any bench row is
  missing from the README table).

## [0.14.3] - 2026-08-09

### Changed

- Fast-path regression fix: the LARGE-ring certifier no longer runs the
  full validator's continuation and the extreme-span extrema are fused
  into the gate accumulator (full pass 5.76-6.06s to 3.75-3.80s, 0
  invalid on the real-world set).

## [0.14.2] - 2026-08-07

### Added

- `geo-traits` feature: `make_valid_geometry`, `is_valid_geometry`,
  `validate_geometry` over the `geo-traits` `GeometryTrait`/`GeometryCollectionTrait`
  abstractions, plus a `GeoRepairValidation` newtype implementing the geo
  `validation::Validation` interface.
- Differential fuzz harness against GEOS (`tests/geos_differential.rs`).
- Fuzzing targets: `fuzz/` with `make_valid`, `validate`, `wkt_repair`,
  `wkb_repair`, and `roundtrip` entry points and a committed corpus; CI
  smoke run for each target.
- CI jobs: `cargo-semver-checks` (API compatibility gate), `cargo-audit`,
  `cargo-deny` (license/advisory/source policy in `deny.toml`), rustdoc with
  `-D warnings`, and a Python bindings job (maturin wheel + pytest).
- `docs/ARCHITECTURE.md` documenting the repair pipeline, routing rules,
  NaN/Inf policy, `keep_collapsed` semantics, and the error model.
- CI coverage for MSRV 1.88 and `no_std` builds.
- wasm32 support with runtime tests: `tests/wasm.rs` runs in Node via
  wasm-bindgen-test-runner in CI; the `wasm` feature adds browser fetch
  (synchronous XHR). The mimalloc global-allocator use site is gated to
  non-wasm targets.
- `io-gpkg` is now a DEFAULT feature (bundled SQLite, module gated out
  on wasm32); `geo_repair::io::load("*.gpkg")` works out of the box.
- `BENCH_OUTPUT=<path.gpkg>`: the real-world bench exports the repaired
  full pass as a QGIS-ready GPKG (original input order preserved).
- **C FFI maturity** (`ffi` feature): every result now carries a
  `GeoRepairErrorCode` (None/Parse/InvalidInput/InvalidGeometry/Encode/
  Panic) so C callers branch programmatically; WKT entry points
  (`geo_repair_make_valid_wkt`, `geo_repair_validate_wkt`,
  `geo_repair_validate_and_fix_wkt`, ...) over a new
  `GeoRepairStringResult`; a batch API
  (`geo_repair_make_valid_batch`, parallel flag, per-item error
  semantics, `GeoRepairWkbBuffer`); `staticlib` added to crate-type for
  static linking. Struct layouts and error codes are ABI-frozen from
  this version. New test layers: `tests/ffi.rs` (25 runtime tests
  exercised in CI on every platform) and `tests/c/test_geo_repair.c` (a
  dependency-free C harness compiled and run against the built library
  on Linux, Windows, and macOS).
- **Python bindings maturity** (`python` feature): complete WKB + WKT
  surface parity - `repair_*`, `repair_*_batch`, `par_repair_*_batch`
  (both formats), `repair_validate_*(_batch)`, `is_valid_*(_batch)`,
  `validate_*(_batch)`, `validate_and_fix_*(_batch)` (WKT batch forms
  added), `keep_collapsed` on every config-taking function, and
  `version()`. abi3 wheels (`pyo3 abi3-py38` -> `cp38-abi3`, one wheel
  per platform serves Python 3.8+); typing stubs shipped in the wheel
  (`geo_repair/__init__.pyi` + `py.typed` + package `__init__.py` shim);
  Production/Stable classifier. `tests/test_python.py` expanded from
  17 WKT-only tests to 41 covering the full surface.
- `docs/BINDINGS.md`: full API reference for both bindings. README
  gained Python and C API sections.

### Changed

- `Cargo.lock`: crossbeam-epoch 0.9.18 -> 0.9.20 (RUSTSEC-2026-0204).
- `[profile.release]` now uses `panic = "unwind"` (was "abort"): the C
  FFI's documented panic containment relies on `catch_unwind`, which is
  dead under abort - a release-built FFI would abort the host process on
  any internal panic. The bench profile keeps `panic = "abort"`.
- GEOS XML suite pinned to GEOS 24ec89dc3; the three remaining
  validator parity gaps closed with index-based checks: disconnected
  interior via ring-touch graph cycle detection (exact orient2d == 0
  touches only), nested holes sharing boundary vertices (incident-
  segment topology port), and a MultiPolygon component crossing
  another component's hole (cross-component ring pairs with
  hole-in-shell-FILL probes). Suite now 937/937 dispatched, 3,629
  overlay cases skipped, 0 unparseable.
- Validation performance (2026-08-06): per-hole shell checks moved to
  one shared shell-edge tree with exact touch probes - full-dataset
  validation 18.06s -> 4.15s (2.6 us/poly, 1.08x GEOS isValid);
  radix-sorted sweep replaces the per-ring R-tree for giants (320ms ->
  50ms); size-partitioned two-pass batch with round-robin interleave.
  Full pass: 3.8-4.0s (1.16x GEOS).
- Documentation drift fixed: GEOS suite counts in README/RELEASE.md/
  lib.rs updated to the measured 937/937 + 213 masked + 3,629 skipped
  (were 934/209/1,565 and a stale "2490/2490" lib.rs claim); RELEASE.md
  fuzz gate corrected (cargo-fuzz has no UBSan; 5 smokes + ASan).

### Fixed

- Zero-orient false positive in the proper-crossing predicate: the product
  form `o1 * o2 < 0` treated a `-0.0` orient (an exact collinear touch,
  common on snapped vertices) as a crossing. Both the full validator
  (`edges_intersect_general`) and the sweep (`segments_properly_cross`,
  `has_no_intersections`) now use a zero-safe strict opposite-sign test.
  This had inflated the real-world invalid count on the 1,579,030-part
  dataset from the true 1 non-winding defect to 2,298, and made the light
  validity gate flag every Structure output.
- `has_no_intersections` no longer skips the closing edge pair, catching
  collinear backtracking closures (a genuine self-intersection class).
- Light-gate hole containment (`holes_contained_cheap`): a hole bbox
  exceeding the shell bbox, or two distinct hole vertices touching the
  shell, now routes the polygon to repair instead of passing it through.
  Closes the DisconnectedInteriorRing passthrough-to-empty gap exposed by
  the zero-orient fix.
- Fuzz-found LineString false NotSimple: `check_linestring_self_intersection`
  used a global bbox eps (1e-12 * scale) that inflates to an absolute
  length at large coordinate magnitude (measured: 1e15-scale line, eps =
  1000 units, flagged a vertex 1 unit from another segment as on-edge).
  Non-adjacent pairs now use the ring path's per-pair predicates
  (`edges_intersect_general` with the relative collinear gate +
  segment-local `edges_vertex_on_edge`), plus an explicit vertex-revisit
  check for shared endpoints (GEOS TestSimple "interior intersection at
  vertices" cases).
- Fuzz-found strip demotion of valid slivers: the magnitude-based noise
  gate (EPS * m^2 * n * 8, m = max |coord|) demoted genuine slivers at
  large coordinate magnitude (measured: 1e15-scale ring, real area 5e14,
  computed shoelace below the worst-case cancellation bound). Replaced
  with an exact-collinearity test (robust orient == 0): only rings whose
  stored coordinates lie bit-exactly on one line are demoted, matching
  GEOS IsValid behavior.
- CI: corrected the cargo-deny arguments (explicit `check` subcommand)
  and the cargo-audit action name (`rustsec/audit-action`); fixed the
  `is_multiple_of` lint site in the GML reader found by the CI's newer
  clippy (rust 1.97).
- Fuzz harness profile: the crate's `panic = "abort"` release profile
  made `catch_unwind` dead in the cargo-fuzz build, so the library's own
  containment guards (arrange `build_cdt_safe`) never ran and the spade
  CDT "vertex insertion failed: TooLarge" panic on mixed-magnitude
  inputs escaped as a libFuzzer deadly signal. The fuzz workspace now
  overrides `[profile.release] panic = "unwind"`; the smoke + ASan
  passes exercise the real containment path.
- Fuzz-found CDT panic (crash during corpus replay): spade rejects
  coordinates whose magnitude exceeds its internal grid
  (`InsertionError::TooLarge`); `cdt::build` previously unwrapped the
  insertion and panicked. With panic = "abort" builds that panic killed
  the process. Insert errors now map to
  `MakeValidError::ConstraintFailure` and the existing `build_cdt_safe`
  fallback routes to the boolean path, so repair succeeds instead of
  aborting. Panic containment is a backstop, not the primary mechanism.
- Fuzz-found valid-polygon destruction (crash-eaab5472): three
  degeneracy gates compared extents against the ring's MAX BBOX SPREAD,
  so one distant coordinate dominated the others. A valid ring with a
  4.9e208 spike and a 1-unit base (8 ULPs at 1e15, fully representable)
  was emptied by the make_valid bbox pre-gate, then demoted to a
  LINESTRING by strip_degenerate, in every repair mode - while both our
  validator and GEOS IsValid keep it. All three gates (make_valid
  pre-gate, strip bbox_ok, has_sub_ulp_edge) now use per-axis/per-edge
  LOCAL thresholds (extent vs the coordinate rounding at that axis's own
  magnitude), and `arrange_chain` passes valid inputs through unchanged.
  Valid in, polygonal out is now enforced by regression tests for both
  fuzz-discovered rings.
- Fuzz-found WKT parser panics (crash-5909cb82): input ending
  mid-document reached unchecked `self.s[self.i]` sites in the reader
  (ring loop, multipolygon loop, parse_point peek, EMPTY checks) and
  panicked with index-out-of-bounds. All sites now guard `i < len`;
  truncated documents return `Err(WktError)`. Regression test covers the
  crash input plus 15 truncation classes; artifact committed to the
  wkt_repair corpus.
- Parser hardening follow-up: the element-loop audit found the same
  comma-then-EOF index bug in three more WKT sites (GEOMETRYCOLLECTION
  loop, MULTILINESTRING loop, MULTIPOLYGON inner ring loop - a trailing
  comma advanced `i` past the buffer end before the EMPTY check
  indexed). All guarded; truncation regression extended with those
  cases. Both readers also gained a 256-level nesting cap (WKB and WKT
  recurse per container level; a crafted document could previously
  overflow the stack - an uncatchable abort). Deep-nesting seeds added
  to both fuzz corpora.
- Fuzz-found WKB OOM abort: a crafted count field (MultiPoint/ring/
  geometry count) drove `Vec::with_capacity` into a 120 GB allocation
  that aborted the process - uncatchable by panic containment. All
  count-driven allocations now go through `read_bounded_count`, which
  rejects counts that cannot fit the remaining buffer as
  `UnexpectedEof` (a count promising more elements than the buffer
  holds IS truncation; no new public error variant at patch level). A
  new `wkb_repair` fuzz target runs the parse + repair pipeline over
  arbitrary bytes in CI (OOM-class seeds committed); the parser stress
  regression runs 200k random buffers plus every truncation of valid
  documents through both readers under panic containment.
- FFI empty-output contract: `GeoRepairResult::success`/batch success
  now return null pointers for empty outputs (the previous dangling
  Vec pointer violated the "non-null iff len > 0" contract).
- Python wheel packaging: `python/geo_repair/__init__.py` (the
  extension re-export shim) was excluded from wheels because the repo's
  `*.py` gitignore rule also drives maturin's source walker. Added a
  `!python/geo_repair/__init__.py` negation so the shim ships (and is
  committed).

## [0.14.1] - 2026-08-03

### Changed

- README refresh for crates.io: compressed validator-gap narrative,
  removed validator-comparison and giant-anatomy sections.

## [0.14.0] - 2026-08-03

### Added

- `docs.rs` metadata for publish; lockfile version sync.
- Documented the validator strictness policy: exact Shewchuk predicates
  plus a deliberate noise gate (collinear epsilon), and the exact
  predicate divergence from GEOS in the light-gate fast path.

### Fixed

- GeoPackage I/O now strips the GP binary blob header before WKB parsing;
  the real-world benchmark reads the original `data_0.gpkg`.

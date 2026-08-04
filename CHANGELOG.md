# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.14.2] - 2026-08-03

### Added

- `geo-traits` feature: `make_valid_geometry`, `is_valid_geometry`,
  `validate_geometry` over the `geo-traits` `GeometryTrait`/`GeometryCollectionTrait`
  abstractions, plus a `GeoRepairValidation` newtype implementing the geo
  `validation::Validation` interface.
- Differential fuzz harness against GEOS (`tests/geos_differential.rs`).
- Fuzzing targets: `fuzz/` with `make_valid`, `validate`, and `wkt_repair`
  entry points and a committed corpus; CI smoke run for each target.
- CI jobs: `cargo-semver-checks` (API compatibility gate), `cargo-audit`,
  `cargo-deny` (license/advisory/source policy in `deny.toml`), rustdoc with
  `-D warnings`, and a Python bindings job (maturin wheel + pytest).
- `docs/ARCHITECTURE.md` documenting the repair pipeline, routing rules,
  NaN/Inf policy, `keep_collapsed` semantics, and the error model.
- CI coverage for MSRV 1.88 and `no_std` builds.

### Changed

- `Cargo.lock`: crossbeam-epoch 0.9.18 -> 0.9.20 (RUSTSEC-2026-0204).

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
  vertices" cases). Restores 934/934 on the GEOS XML suite.
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
  overrides `[profile.release] panic = "unwind"`; the smoke + ASan +
  UBSan passes exercise the real containment path.
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
- Python bindings: tests rewritten to the WKT surface (17 tests green;
  GeoJSON binding references removed with the deleted bindings).

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

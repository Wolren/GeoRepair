# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.15.0] - 2026-08-03

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

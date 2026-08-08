//! Validate and repair invalid OGC GIS geometries in Rust.
//!
//! Detects and fixes geometry defects — self-intersections, unclosed rings,
//! degenerate shapes, NaN coordinates, and more — using algorithms selected
//! by geometry type. The **Structure** strategy (default) mirrors GEOS's
//! ST_MakeValid algorithm; the **Arrange** strategy uses CDT-based repair
//! as a robust fallback for complex topologies. Passes 937/937 dispatched
//! GEOS XML validation cases (213 documented masked divergences), with
//! parallel batch performance **1.16× GEOS** wall time on the 1.58M
//! polygon real-world dataset (measured 2026-08-06).
//!
//! # Quick start
//!
//! ```toml
//! [dependencies]
//! geo-repair = "0.14"
//! ```
//!
//! ```rust
//! # use geo::{Geometry, Point};
//! # let geometry = Geometry::Point(Point::new(0.0, 0.0));
//! use geo_repair::{is_valid, validate, MakeValid, ValidateAndFix};
//! use geo_repair::{read_wkb, write_wkb, read_wkt, write_wkt};
//!
//! // Check validity
//! let result = validate(&geometry);
//! if !result.valid {
//!     for err in &result.errors {
//!         eprintln!("  {err}");
//!     }
//! }
//!
//! // Fix invalid geometry
//! let fixed = geometry.make_valid();
//!
//! // Combined validate-and-fix
//! let (result, fixed) = geometry.validate_and_fix();
//!
//! // WKB roundtrip
//! let bytes: Vec<u8> = write_wkb(&geometry);
//! let geom = read_wkb(&bytes).unwrap();
//!
//! // WKT roundtrip
//! let text: String = write_wkt(&geometry);
//! let geom = read_wkt(&text).unwrap();
//! ```
//!
//! ## With method selection
//!
//! ```rust
//! # use geo::{Geometry, Point};
//! # let geometry = Geometry::Point(Point::new(0.0, 0.0));
//! use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
//!
//! let config = MakeValidConfig {
//!     poly_method: PolyMethod::Arrange,
//!     keep_collapsed: false,
//!     ..Default::default()
//! };
//! let fixed = geometry.make_valid_with_config(&config);
//! ```
//!
//! ## Integration with the geo ecosystem
//!
//! geo-repair is built on `geo` types and participates in the georust
//! ecosystem through two layers.
//!
//! ### geo-traits sources (feature `geo-traits`)
//!
//! The [`interop`] module exposes the engines over
//! `geo_traits::GeometryTrait` / `geo_traits::GeometryCollectionTrait`, the
//! trait layer implemented by `geo`, geoarrow, geozero, and the `wkb` crate.
//! Any geometry source that implements those traits can be validated or
//! repaired in one call; geo types materialize internally, and the result
//! comes back as `geo::Geometry<f64>`.
//!
//! ```rust
//! # #[cfg(feature = "geo-traits")] {
//! use geo::{Geometry, LineString, Polygon};
//! use geo_repair::interop::{is_valid_geometry, make_valid_geometry, validate_geometry};
//!
//! // Any geo-traits source works: here a geo Polygon, but a wkb or
//! // geoarrow geometry would do the same.
//! let bowtie = Geometry::Polygon(Polygon::new(
//!     LineString::from(vec![(0.0, 0.0), (10.0, 10.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0)]),
//!     vec![],
//! ));
//! assert!(!is_valid_geometry(&bowtie));
//! let fixed = make_valid_geometry(&bowtie);
//! assert!(validate_geometry(&fixed).valid);
//! # }
//! ```
//!
//! Batch repair over a collection (`geo::GeometryCollection`, geoarrow
//! arrays, or any `GeometryCollectionTrait`):
//!
//! ```rust
//! # #[cfg(feature = "geo-traits")] {
//! use geo::{Geometry, GeometryCollection, LineString, Polygon};
//! use geo_repair::interop::make_valid_geometries;
//!
//! let collection = GeometryCollection(vec![
//!     Geometry::Polygon(Polygon::new(
//!         LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
//!         vec![],
//!     )),
//!     Geometry::Polygon(Polygon::new(
//!         LineString::from(vec![(0.0, 0.0), (10.0, 10.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0)]),
//!         vec![],
//!     )),
//! ]);
//! let repaired = make_valid_geometries(&collection);
//! assert_eq!(repaired.len(), 2);
//! # }
//! ```
//!
//! ### geo's `Validation` trait (always available)
//!
//! geo's `Validation` trait and `Invalid*` error enums are the ecosystem's
//! validation vocabulary, but the orphan rule prevents implementing geo's
//! trait for geo's own types. The [`GeoRepairValidation`] adapter wraps a
//! `&Geometry<f64>` and exposes geo_repair's stricter engine through geo's
//! trait: code that calls `.is_valid()`, `.check_validation()`, or
//! `.validation_errors()` runs geo_repair's validator, including the
//! deliberate strictness gates that geo's exact-only validator lacks.
//!
//! ```rust
//! # use geo::{Geometry, LineString, Polygon};
//! use geo::algorithm::validation::Validation;
//! use geo_repair::GeoRepairValidation;
//!
//! let bowtie = Geometry::Polygon(Polygon::new(
//!     LineString::from(vec![(0.0, 0.0), (10.0, 10.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0)]),
//!     vec![],
//! ));
//! let adapter = GeoRepairValidation(&bowtie);
//! assert!(!adapter.is_valid());
//! let errors = adapter.validation_errors();
//! assert_eq!(errors.len(), 1);
//! # let _ = errors;
//! ```
//!
//! The mapping is best-effort: geo_repair checks 18 OGC rules while geo's
//! error enums cover a subset, and geo carries ring/index payloads that
//! geo_repair's errors do not record. Unmappable classes (ring closure,
//! pinch points, nested holes, orientation, collinearity, duplicates) are
//! omitted from the geo view; use [`validate`] and [`validate_reason`] for
//! the complete report.
//!
//! ## WKB I/O (built-in, no dependencies)
//!
//! ```rust
//! # use geo::{Geometry, Point};
//! # let geom = Geometry::Point(Point::new(0.0, 0.0));
//! # let concat_buffer = vec![];
//! use geo_repair::{read_wkb, write_wkb, read_wkb_concat};
//!
//! let wkb: Vec<u8> = write_wkb(&geom);
//! let geom = read_wkb(&wkb).unwrap();
//! let geoms: Vec<Geometry<f64>> = read_wkb_concat(&concat_buffer).unwrap();
//! ```
//!
//! ## WKT I/O (built-in, no dependencies)
//!
//! ```rust
//! # use geo::{Geometry, Point};
//! # let geom = Geometry::Point(Point::new(0.0, 0.0));
//! use geo_repair::{read_wkt, write_wkt};
//!
//! let wkt: String = write_wkt(&geom);
//! let geom = read_wkt(&wkt).unwrap();
//! ```
//!
//! ## Binary format loading (custom `.bin` format, fast bulk I/O)
//!
//! ```rust,no_run
//! use geo_repair::load_bin;
//!
//! let polys: Vec<geo::Polygon<f64>> = load_bin("dataset.bin").unwrap();
//! ```
//!
//! # Feature flags
//!
//! | Feature | Description | Default |
//! |---------|-------------|---------|
//! | `std` | Standard library + file I/O. Disable for no_std builds. | yes |
//! | `arrange` | CDT-based polygon repair (requires `spade`) | yes |
//! | `structure` | Structure-based fast-path repair | yes |
//! | `parallel` | Rayon parallel processing (non-WASM) | yes |
//! | `simd` | Retained for compatibility; stable builds use auto-vectorized scalar kernels (hand-written AVX2 measured slower) | yes |
//! | `simd-portable` | Portable SIMD via `core::simd` (nightly) | no |
//! | `validate` | OGC validation predicates | yes |
//! | `memmap` | Memory-mapped binary file loading | no* |
//! | `wasm` | WASM browser fetch (synchronous XHR, `wasm::fetch_geometry`) | wasm32 only |
//! | `mimalloc` | Use mimalloc global allocator | yes |
//! | `io-shp` | Shapefile format backend | no |
//! | `io-wkb` | No-op (WKB is always compiled in) | — |
//! | `io-wkt` | No-op (WKT is always compiled in) | — |
//! | `io-csv` | CSV format backend | no |
//! | `io-gml` | GML/XML format backend | no |
//! | `io-gpkg` | GeoPackage format backend (default; not on wasm32) | yes |
//! | `io-all` | All opt-in backends except gpkg | no |
//! | `io-all-native` | All opt-in backends including gpkg | no |
//! | `ffi` | C-compatible FFI bindings | no |
//! | `geo-traits` | Interop surface over `geo_traits::GeometryTrait` / `GeometryCollectionTrait` (geo, geoarrow, geozero, wkb sources) | no |
//! | `python` | Python bindings via PyO3 | no |
//! | `proj` | CRS transformation via PROJ (requires the native PROJ library; mutually exclusive with `io-gpkg` on the sqlite3 link) | no |
//! | `serde` | Geometry serde support | no |
//! | `bench-geos` | GEOS comparison benchmarks (static — MSVC, no LTO) | no |
//! | `bench-geos-system` | GEOS comparison benchmarks (system — conda LLVM, full LTO) | no |
//!
//! *`memmap` was default in 0.10 but moved to opt-in in 0.11.
//!
//! # Platform support
//!
//! | Platform | Core | SIMD | I/O | Parallel | Python |
//! |----------|------|------|-----|----------|--------|
//! | x86_64 Windows/Linux/macOS | Yes | Auto-vectorized scalar | Yes | Yes | Yes |
//! | aarch64 macOS/Linux | Yes | Auto-vectorized scalar | Yes | Yes | Yes |
//! | WASM32 | Yes | Auto-vectorized scalar | In-memory only | No | No |
//! | no_std (embedded) | Yes | Auto-vectorized scalar | No | No | No |
//!
//! SIMD is provided by LLVM's auto-vectorizer on all platforms; no special
//! RUSTFLAGS are required. Hand-written AVX2 intrinsics were measured and
//! removed (see `simd/mod.rs`).
//!
//! # no_std
//!
//! Disable the `std` feature for no_std builds. Core validation, repair,
//! and WKB parsing work without std. File I/O, parallel processing, and
//! Python/FFI bindings require std:
//!
//! ```shell
//! cargo check --no-default-features --features arrange,structure,simd
//! ```
//!
//! # Validation
//!
//! The [`GeoValidation`] trait checks 18 OGC validity rules using Shewchuk
//! adaptive-precision orientation tests (via the `robust` crate):
//!
//! | Rule | Applies to |
//! |------|-----------|
//! | Coordinate finiteness | All geometries |
//! | Ring closure | Polygon rings |
//! | Ring minimum vertices (≥4) | Polygon rings |
//! | Ring self-intersection | Polygon rings |
//! | Pinch points (non-consecutive duplicates) | Rings |
//! | Hole containment (inside shell) | Polygon |
//! | No nested holes | Polygon |
//! | Interior ring connectivity | Polygon |
//! | Ring orientation (exterior CCW, interior CW) | Polygon |
//! | Non-collinear rings | Polygon |
//! | Consecutive duplicates | Lines/rings |
//! | Duplicate rings | Polygon |
//! | Duplicate points | MultiPoint |
//! | Duplicate lines | MultiLineString |
//! | Non-zero-length lines | Line |
//! | Non-degenerate exterior | Polygon |
//! | Simplicity (no interior intersections) | LineString, MultiLineString |
//! | Nesting depth limit | GeometryCollection |
//!
//! ```rust
//! # use geo::{Geometry, Point};
//! # let geom = Geometry::Point(Point::new(0.0, 0.0));
//! use geo_repair::{is_valid, validate, validate_reason, GeoValidation, ValidationResult};
//!
//! let ok: bool = geom.is_valid();
//! let ok2: bool = is_valid(&geom);
//!
//! let result: ValidationResult = validate(&geom);
//! let reason: String = validate_reason(&geom);
//! ```
//!
//! # Polygon repair strategies
//!
//! | Strategy | Approach | Strengths | Weaknesses |
//! |----------|----------|-----------|------------|
//! | **Arrange** (CDT) | Constrained Delaunay triangulation → face labeling → ring extraction | Handles any topology. No self-intersection limit. | Slower, especially on large rings. Requires `spade`. |
//! | **Structure** (fast path) | Planar graph extraction → face walking → winding-number assembly | 10-100× faster for valid/simple inputs. No external deps. | Falls back on complex topologies (many holes, nested self-intersections). |
//!
//! **Auto** (default) tries Structure first and falls back to Arrange.
//! The repair pipeline enforces OGC-correct winding order (CCW exterior,
//! CW interior) on all output.
//!
//! # CRS support
//!
//! The [`Crs`] type stores EPSG codes and provides CRS-aware tolerance heuristics:
//! - Geographic (lon/lat): `1e-10` degrees
//! - Projected (metres): `1e-6` metres
//! - Unknown: `1e-12`
//!
//! CRS is set directly on [`MakeValidConfig`]:
//!
//! ```rust
//! use geo_repair::{Crs, MakeValidConfig};
//!
//! let config = MakeValidConfig {
//!     crs: Some(Crs::from_epsg(4326)),
//!     ..Default::default()
//! };
//! ```
//!
//! # I/O
//!
//! GeoRepair provides format-agnostic dispatch and individual backends:
//!
//! ```rust,no_run
//! # use geo::{Geometry, Point};
//! # let geom = Geometry::Point(Point::new(0.0, 0.0));
//! # let concat_buffer = vec![];
//! use geo_repair::{
//!     diagnose_file, load, load_bin, read_wkb, read_wkb_concat, read_wkb_from,
//!     read_ewkb, write_ewkb, EwkbGeometry, EwkbDims,
//!     read_wkt, read_wkt_from, write_wkt, write_wkt_to, infer_wkt_type,
//!     write_wkb, write_wkb_to, write_wkb_with_opts, Endianness, WriteOptions,
//!     repair_file, save, MakeValidConfig,
//! };
//!
//! let geoms = load("input.wkb").unwrap();
//! let geoms = read_wkb_concat(&concat_buffer).unwrap();
//!
//! for result in diagnose_file("input.bin").unwrap() {
//!     println!("{}", result.reason());
//! }
//!
//! repair_file("invalid.wkb", "fixed.wkb", &MakeValidConfig::default()).unwrap();
//! save("output.wkt", &geoms[0]).unwrap();
//!
//! // Standard LE WKB
//! let wkb: Vec<u8> = write_wkb(&geom);
//! // Big-endian WKB
//! let be_wkb: Vec<u8> = write_wkb_with_opts(&geom, &WriteOptions { endianness: Endianness::BigEndian });
//! // Write to any io::Write target
//! write_wkb_to(&geom, &mut std::io::stdout()).unwrap();
//! // Read from any io::Read source
//! let geom = read_wkb_from(&wkb[..]).unwrap();
//!
//! // EWKB with SRID and Z/M preservation
//! let ewkb = EwkbGeometry {
//!     geometry: geom.clone(),
//!     srid: Some(4326),
//!     dims: EwkbDims::XYZ,
//!     extra_coords: vec![100.0],
//! };
//! let ewkb_bytes = write_ewkb(&ewkb);
//! let back = read_ewkb(&ewkb_bytes).unwrap();
//!
//! // WKT with streaming I/O
//! let wkt: String = write_wkt(&geom);
//! let geom = read_wkt(&wkt).unwrap();
//! write_wkt_to(&geom, &mut std::io::stdout()).unwrap();
//! let geom = read_wkt_from(wkt.as_bytes()).unwrap();
//!
//! // Peek at WKT type without parsing
//! let (type_name, _dims) = infer_wkt_type("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))").unwrap();
//!
//! let polys = load_bin("dataset.bin").unwrap();
//! ```
//!
//! | Extension | Format | Backend |
//! |-----------|--------|---------|
//! | `.wkb` / `.wks` | WKB (LE/BE, EWKB SRID, Z/M variants, io::Read/Write) | Zero-dep built-in |
//! | `.bin` | Custom binary bulk polygon format | Zero-dep built-in |
//! | `.shp` | Shapefile | `io-shp` feature |
//! | `.wkt` | WKT (io::Read/Write, type inference) | Zero-dep built-in |
//! | `.csv` | CSV with WKT geometry | `io-csv` feature |
//! | `.gml` | GML/XML | `io-gml` feature |
//! | `.gpkg` | GeoPackage (SQLite) | `io-gpkg` feature |
//!
//! # Geometry type coverage
//!
//! | Geometry | Repair approach |
//! |----------|----------------|
//! | `Polygon` / `MultiPolygon` | Structure fast path or Arrange CDT fallback |
//! | `LineString` / `MultiLineString` | NaN filtering, duplicate removal, self-intersection noding |
//! | `Line` | Zero-length and NaN detection |
//! | `Point` / `MultiPoint` | NaN/Inf filtering, deduplication |
//! | `Rect` / `Triangle` | Basic degeneracy checks |
//! | `GeometryCollection` | Recursive repair of children |
//!
//! # Known limitations
//!
//! - **CDT arranger may panic** on certain degenerate inputs (all-collinear
//!   exterior rings, coordinates near f64::MAX). This is a known limitation
//!   of `spade`.
//! - **OGC compliance** is a key goal but not yet formally certified. The
//!   validation module checks 18 OGC predicates and passes 937/937 dispatched GEOS XML
//!   tests.
//! - **GeometryCollection cross-component intersection** is not validated.
//! - **Z/M coordinate consistency** is not validated.
//!
//! # License
//!
//! Apache-2.0
#![cfg_attr(feature = "simd-portable", feature(portable_simd))]
//! # no_std
//!
//! Disable the default `std` feature for an alloc-only build. WKB/WKT
//! parsing, validation, and repair work without `std`; only file I/O
//! (`.gpkg`/`.csv`/`.shp`/`.gml` loaders, binary file helpers) requires
//! it. The crate itself is std-free: the `geo` dependency still links
//! `std` on hosted targets, so embedded (bare-metal) targets are not yet
//! supported — see geo-types for a fully embedded geometry type layer.
#![cfg_attr(not(feature = "std"), no_std)]
#[macro_use]
extern crate alloc;

/// Compile-time guard: ensures `cfg(feature = "rstar")` is active when any
/// rstar-dependent feature is enabled. Prevents silent O(n²) regression when
/// `dep:rstar` is used in Cargo.toml feature lists instead of the explicit
/// `rstar = ["dep:rstar"]` feature alias.
#[cfg(any(feature = "arrange", feature = "structure", feature = "validate"))]
const _: () = {
    #[cfg(not(feature = "rstar"))]
    compile_error!(
        "cfg(feature = \"rstar\") must be set when rstar-dependent features are active.\n  \
         Fix: replace `dep:rstar` with `rstar` in Cargo.toml feature lists. The pattern is:\n  \
         rstar = [\"dep:rstar\"]  # explicit feature alias\n  \
         foo = [\"rstar\"]          # not foo = [\"dep:rstar\"]"
    );
};

/// Core configuration types for geometry repair.
pub mod core;
/// Coordinate reference system (CRS) handling and transformation.
pub mod crs;
/// Double-double arithmetic for robust geometric computations.
pub mod dd;
/// Feature metadata associated with geometries.
pub mod feature;
/// Geometry I/O: WKB, binary format, and format-dispatch helpers.
pub mod io;
/// Geometry repair implementation via the [`MakeValid`] trait.
pub mod make_valid;
/// Ring orientation utilities (CW/CCW winding).
pub mod orient;
/// Coordinate snapping to a precision grid.
pub mod snap;
pub(crate) mod util;
/// OGC Simple Features geometry validation predicates.
pub mod validation;
/// WASM browser fetch (`wasm` feature; wasm32 targets only).
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub mod wasm;
/// Z/M coordinate value preservation through the repair pipeline.
pub mod zm;

#[cfg(feature = "geo-traits")]
/// geo-traits interop: validate and repair any `GeometryTrait` /
/// `GeometryCollectionTrait` source (geo, geoarrow, geozero, wkb).
pub mod interop;

#[cfg(feature = "arrange")]
/// CDT-based polygon repair for complex topologies (Arrange strategy).
pub mod arrange;
/// Tolerance-based repeated-point removal (GEOS RepeatedPointRemover parity).
pub mod cleanup;
/// Segment noding: intersection detection, snap-rounding, and validation.
pub mod noding;
/// Geometry precision reduction with topology preservation.
pub mod reduce;
#[cfg(feature = "structure")]
/// GEOS-compatible fast-path polygon repair via planar graph extraction.
pub mod structure;

/// Language bindings: C FFI and Python (PyO3).
#[cfg(any(feature = "ffi", feature = "python"))]
pub mod bindings;
#[cfg(feature = "ffi")]
pub use bindings::ffi;
#[cfg(feature = "python")]
pub use bindings::python;
#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
/// Rayon-based parallel batch geometry repair.
pub mod parallel;
/// Geometric predicates (orientation, point-in-ring, AABB, snapping).
/// Compiled unconditionally; the `simd` feature is retained for backward
/// compatibility and gates the nightly `simd-portable` path. Stable builds
/// use scalar kernels that LLVM auto-vectorizes — hand-written AVX2 was
/// measured slower and removed (see `simd/mod.rs`).
pub mod simd;

/// Tolerance-based repeated-point removal.
pub use cleanup::{remove_repeated_coords, remove_repeated_points};
/// Repair configuration, error types, and polygon method selection.
pub use core::{MakeValidConfig, MakeValidError, PolyMethod};
/// Coordinate reference system wrapper.
pub use crs::Crs;
/// A feature combining geometry with optional attributes and CRS.
pub use feature::Feature;
pub use io::{
    Endianness, EwkbDims, EwkbGeometry, WkbError, WktError, WriteOptions, infer_wkt_type,
    read_ewkb, read_wkb, read_wkb_concat, read_wkt, write_ewkb, write_wkb, write_wkb_with_opts,
    write_wkt,
};
#[cfg(feature = "std")]
pub use io::{
    diagnose_file, load, load_bin, load_bin_stream, read_wkb_from, read_wkt_from, repair_file,
    save, write_wkb_to, write_wkt_to,
};
/// Trait for repairing invalid geometries.
pub use make_valid::MakeValid;
#[cfg(any(feature = "arrange", feature = "structure"))]
/// Combines validation and repair in a single step, returning errors for
/// violations that could not be automatically repaired.
pub use make_valid::ValidateAndFix;
/// Coordinate snapping functions.
pub use snap::{DEFAULT_GRID, snap_coord, snap_coord_default, snap_line, snap_lines};
/// OGC validation predicates and result types.
pub use validation::{
    GeoRepairValidation, GeoValidation, GeometryValidationError, ValidationResult, is_valid,
    map_geo_invalid, validate, validate_reason,
};

// mimalloc is a non-wasm target-gated dependency (bundled C allocator):
// the feature is in the defaults, so the use site must be gated too or a
// default-features wasm build fails with E0433 (measured 2026-08-06).
#[cfg(all(feature = "mimalloc", not(target_arch = "wasm32")))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Profile each step of the structure fast path on a sample of polygons.
/// Prints timing breakdown and pass rates to stderr.
// Native-only: it times with `std::time::Instant`, which is not
// implemented on wasm32-unknown-unknown (panics on call).
#[cfg(all(
    feature = "arrange",
    feature = "structure",
    feature = "std",
    not(target_arch = "wasm32")
))]
pub fn profile_structure_fastpath(polys: &[geo::Polygon<f64>], sample: usize) {
    use geo::LinesIter;
    use std::time::Instant;

    let n = polys.len().min(sample);
    let mut t_basic = 0.0f64;
    let mut t_collect = 0.0f64;
    let mut t_int = 0.0f64;
    let mut t_holes = 0.0f64;
    let mut n_basic_pass = 0usize;
    let mut n_int_pass = 0usize;
    let mut n_holes_pass = 0usize;

    for p in &polys[..n] {
        let t0 = Instant::now();
        let basic = arrange::poly_has_basic_form(p);
        t_basic += t0.elapsed().as_secs_f64();
        if !basic {
            continue;
        }
        n_basic_pass += 1;

        let t0 = Instant::now();
        let lines: Vec<_> = p.lines_iter().collect();
        t_collect += t0.elapsed().as_secs_f64();
        if lines.is_empty() {
            continue;
        }

        let t0 = Instant::now();
        let no_int = arrange::prep::has_no_intersections(&lines);
        t_int += t0.elapsed().as_secs_f64();
        if !no_int {
            continue;
        }
        n_int_pass += 1;

        let t0 = Instant::now();
        let holes = arrange::holes_are_valid(p);
        t_holes += t0.elapsed().as_secs_f64();
        if !holes {
            continue;
        }
        n_holes_pass += 1;
    }

    let total = t_basic + t_collect + t_int + t_holes;
    eprintln!("\n═══ Structure fast path profile (n={n}) ═══");
    eprintln!(
        "  poly_has_basic_form:  {:.3}s ({:.1}%)",
        t_basic,
        t_basic / total * 100.0
    );
    eprintln!(
        "  lines collect:        {:.3}s ({:.1}%)",
        t_collect,
        t_collect / total * 100.0
    );
    eprintln!(
        "  has_no_intersections: {:.3}s ({:.1}%)",
        t_int,
        t_int / total * 100.0
    );
    eprintln!(
        "  holes_are_valid:      {:.3}s ({:.1}%)",
        t_holes,
        t_holes / total * 100.0
    );
    eprintln!("  ─────");
    eprintln!("  total:                {:.3}s", total);
    eprintln!(
        "  basic form pass:      {n_basic_pass}/{n} ({:.1}%)",
        n_basic_pass as f64 / n as f64 * 100.0
    );
    eprintln!(
        "  no intersections:     {n_int_pass}/{n} ({:.1}%)",
        n_int_pass as f64 / n as f64 * 100.0
    );
    eprintln!(
        "  holes valid:          {n_holes_pass}/{n} ({:.1}%)",
        n_holes_pass as f64 / n as f64 * 100.0
    );
    eprintln!();
}

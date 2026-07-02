//! Validate and repair invalid OGC GIS geometries in Rust.
//!
//! Detects and fixes geometry defects — self-intersections, unclosed rings,
//! degenerate shapes, NaN coordinates, and more — using algorithms selected
//! by geometry type. The **Structure** strategy (default) mirrors GEOS's
//! ST_MakeValid algorithm; the **Arrange** strategy uses CDT-based repair
//! as a robust fallback for complex topologies. Passes 2490/2490 GEOS XML
//! validation tests, with parallel batch performance **0.30× GEOS** (3.3×
//! faster) on 1.58M data set polygons.
//!
//! # Quick start
//!
//! ```toml
//! [dependencies]
//! geo-repair = "0.12"
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
//! | `simd` | AVX2-accelerated orientation tests (x86_64) | yes |
//! | `simd-portable` | Portable SIMD via `core::simd` (nightly) | no |
//! | `validate` | OGC validation predicates | yes |
//! | `memmap` | Memory-mapped binary file loading | no* |
//! | `wasm` | WASM browser fetch (synchronous XHR) | no |
//! | `mimalloc` | Use mimalloc global allocator | yes |
//! | `io-shp` | Shapefile format backend | no |
//! | `io-wkb` | No-op (WKB is always compiled in) | — |
//! | `io-wkt` | No-op (WKT is always compiled in) | — |
//! | `io-csv` | CSV format backend | no |
//! | `io-gml` | GML/XML format backend | no |
//! | `io-gpkg` | GeoPackage format backend (not WASM) | no |
//! | `io-all` | All opt-in backends except gpkg | no |
//! | `io-all-native` | All opt-in backends including gpkg | no |
//! | `ffi` | C-compatible FFI bindings | no |
//! | `python` | Python bindings via PyO3 | no |
//! | `proj` | CRS transformation via PROJ | no |
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
//! | x86_64 Windows/Linux/macOS | Yes | Yes (AVX2) | Yes | Yes | Yes |
//! | aarch64 macOS/Linux | Yes | Scalar | Yes | Yes | Yes |
//! | WASM32 | Yes | Scalar | In-memory only | No | No |
//! | no_std (embedded) | Yes | Scalar | No | No | No |
//!
//! AVX2 requires `RUSTFLAGS="-C target-cpu=native"` at build time. Falls
//! back to scalar on CPUs without AVX2 or non-x86_64 targets.
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
//!   validation module checks 18 OGC predicates and passes 2490/2490 GEOS XML
//!   tests.
//! - **GeometryCollection cross-component intersection** is not validated.
//! - **Z/M coordinate consistency** is not validated.
//!
//! # License
//!
//! Apache-2.0
#![cfg_attr(feature = "simd-portable", feature(portable_simd))]


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
/// Z/M coordinate value preservation through the repair pipeline.
pub mod zm;

#[cfg(feature = "arrange")]
/// CDT-based polygon repair for complex topologies (Arrange strategy).
pub mod arrange;
/// Segment noding: intersection detection, snap-rounding, and validation.
pub mod noding;
/// Geometry precision reduction with topology preservation.
pub mod reduce;
#[cfg(feature = "structure")]
/// GEOS-compatible fast-path polygon repair via planar graph extraction.
pub mod structure;

#[cfg(feature = "ffi")]
#[path = "bindings/ffi.rs"]
/// C-compatible FFI bindings for geo-repair.
pub mod ffi;
#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
/// Rayon-based parallel batch geometry repair.
pub mod parallel;
#[cfg(feature = "python")]
#[path = "bindings/python.rs"]
/// Python bindings via PyO3.
pub mod python;
#[cfg(feature = "simd")]
/// AVX2-accelerated geometric predicates (x86_64).
pub mod simd;

/// Repair configuration, error types, and polygon method selection.
pub use core::{MakeValidConfig, MakeValidError, PolyMethod};
/// Coordinate reference system wrapper.
pub use crs::Crs;
/// A feature combining geometry with optional attributes and CRS.
pub use feature::Feature;
pub use io::{
    diagnose_file, infer_wkt_type, load, load_bin, load_bin_stream, read_ewkb, read_wkb,
    read_wkb_concat, read_wkb_from, read_wkt, read_wkt_from, repair_file, save, write_ewkb,
    write_wkb, write_wkb_to, write_wkb_with_opts, write_wkt, write_wkt_to, Endianness, EwkbDims,
    EwkbGeometry, WkbError, WktError, WriteOptions,
};
/// Trait for repairing invalid geometries.
pub use make_valid::MakeValid;
#[cfg(any(feature = "arrange", feature = "structure"))]
/// Combines validation and repair in a single step, returning errors for
/// violations that could not be automatically repaired.
pub use make_valid::ValidateAndFix;
/// Coordinate snapping functions.
pub use snap::{snap_coord, snap_coord_default, snap_line, snap_lines, DEFAULT_GRID};
/// OGC validation predicates and result types.
pub use validation::{
    is_valid, validate, validate_reason, GeoValidation, GeometryValidationError, ValidationResult,
};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Profile each step of the structure fast path on a sample of polygons.
/// Prints timing breakdown and pass rates to stderr.
#[cfg(all(feature = "arrange", feature = "structure"))]
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

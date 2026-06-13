#![cfg_attr(feature = "simd-portable", feature(portable_simd))]
//! Repair invalid GIS geometries using per-type optimal algorithms.
//!
//! For polygons uses Constrained Delaunay Triangulation (Arrange) or
//! Structure fast path; for lines uses noding; for points simply filters
//! NaN/Inf coordinates.
//!
//! # Platform support
//!
//! ```text
//!                 Core¹  SIMD     I/O²    Parallel  Python   C FFI
//! x86_64 (all OS) ✓      AVX2³   ✓       ✓         ✓        ✓
//! aarch64         ✓      scalar  ✓       ✓         ✓        ✓
//! WASM32          ✓      scalar  ✗       ✗         ✗        ✗
//! ```
//!
//! ¹ `arrange` + `structure` features are always available.
//! ² `io-*` features require std::fs and are not available on WASM.
//! ³ Enable with `RUSTFLAGS="-C target-cpu=native"`; scalar fallback otherwise.

// Platform compatibility checks — produce clear errors for unsupported combos.
#[cfg(all(feature = "python", target_arch = "wasm32"))]
compile_error!("Python bindings (feature = \"python\") are not supported on WASM32. PyO3 requires a native CPython runtime.");
#[cfg(all(feature = "ffi", target_arch = "wasm32"))]
compile_error!("C FFI (feature = \"ffi\") is not supported on WASM32. Use the Rust API directly.");
#[cfg(all(feature = "parallel", target_arch = "wasm32"))]
compile_error!(
    "Parallel (feature = \"parallel\") is not supported on WASM32. Rayon requires OS threads."
);
#[cfg(all(feature = "bench-geos", target_arch = "wasm32"))]
compile_error!("GEOS benchmarks (feature = \"bench-geos\") are not supported on WASM32. GEOS is a native C++ library.");
#[cfg(all(
    feature = "python",
    not(any(feature = "arrange", feature = "structure"))
))]
compile_error!(
    "Python bindings require at least one repair backend. Enable 'arrange' or 'structure' feature."
);

// # Quick start
//
// ```ignore
// use geo_repair::MakeValid;
// use geo::{Geometry, Point};
//
// let geom = Geometry::Point(Point::new(1.0, 2.0));
// let fixed = geom.make_valid();
// ```
//
// OGC Simple Features compliance is a key goal. The validation module
// checks core rules (ring closure, orientation, self-intersection,
// hole containment, etc.). The repair module fixes violations.
//
// Supported I/O formats: GeoJSON, WKT, WKB, CSV+WKT, Shapefile,
// GeoPackage (.gpkg), GML (.gml/.xml), and a custom binary format (.bin).

pub mod core;
pub mod crs;
pub mod feature;
#[cfg(not(target_arch = "wasm32"))]
pub mod io;
pub mod make_valid;
pub mod orient;
pub mod snap;
pub mod validation;
pub mod zm;

use geo::Geometry;

#[cfg(feature = "arrange")]
pub mod arrange;
pub mod noding;
#[cfg(feature = "structure")]
pub mod structure;

#[cfg(feature = "ffi")]
#[path = "bindings/ffi.rs"]
pub mod ffi;
#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
pub mod parallel;
#[cfg(feature = "python")]
#[path = "bindings/python.rs"]
pub mod python;
#[cfg(feature = "simd")]
pub mod simd;

/// Configuration for geometry repair (method selection, CRS, etc.).
pub use core::{MakeValidConfig, MakeValidError, PolyMethod};
/// Coordinate Reference System representation.
pub use crs::Crs;
/// A GIS feature with geometry, attributes, CRS, and Z/M values.
pub use feature::Feature;
/// Load geometries from any supported format (auto-detected by extension).
/// Not available on wasm32 targets.
#[cfg(not(target_arch = "wasm32"))]
pub use io::{
    export_features, export_geometries, export_geometries_with_crs, load_features, load_geometries,
    load_geometries_with_crs,
};
/// Trait for repairing invalid geometries.
pub use make_valid::MakeValid;
#[cfg(any(feature = "arrange", feature = "structure"))]
/// Combines validation and repair in a single step, returning errors for
/// violations that could not be automatically repaired.
pub use make_valid::ValidateAndFix;
/// Coordinate snapping utilities for grid-based deduplication.
pub use snap::{snap_coord, snap_coord_default, snap_line, snap_lines, DEFAULT_GRID};
/// OGC geometry validation traits and types.
pub use validation::{GeoValidation, GeometryValidationError, ValidationResult};

/// Check whether a geometry is OGC-valid (convenience wrapper).
pub fn is_valid(geom: &Geometry<f64>) -> bool {
    geom.is_valid()
}

/// Validate a geometry and return detailed error information.
pub fn validate(geom: &Geometry<f64>) -> ValidationResult {
    geom.validate()
}

#[cfg(all(feature = "mimalloc", not(target_arch = "wasm32")))]
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

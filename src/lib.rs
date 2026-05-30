pub mod core;
pub mod io;
pub mod make_valid;
pub mod orient;
pub mod snap;
pub mod validation;

#[cfg(feature = "arrange")]
pub mod arrange;
pub mod noding;
#[cfg(feature = "structure")]
pub mod structure;

#[cfg(feature = "ffi")]
pub mod ffi;
#[cfg(feature = "parallel")]
pub mod parallel;
#[cfg(feature = "simd")]
pub mod simd;

pub use core::{MakeValidConfig, MakeValidError, PolyMethod};
pub use io::load_geometries;
pub use make_valid::MakeValid;
#[cfg(any(feature = "arrange", feature = "structure"))]
pub use make_valid::ValidateAndFix;
pub use validation::{GeoValidation, GeometryValidationError, ValidationResult};

/// Re-export of the upstream `geo::MakeValid` trait, implemented for
/// `Polygon<f64>` and `MultiPolygon<f64>` when the `arrange` or `structure`
/// feature is enabled.
#[cfg(any(feature = "arrange", feature = "structure"))]
pub use geo::MakeValid as GeoMakeValid;

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

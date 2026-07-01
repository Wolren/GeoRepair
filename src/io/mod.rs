//! Format-agnostic geometry I/O: WKB, WKT, and binary format backends.
//!
//! Built-in backends (no extra dependencies):
//! - **WKB** (`read_wkb`, `write_wkb`): OGC Well-Known Binary, handles byte
//!   order, EWKB SRID flags, all geometry types including Z/M variants.
//! - **WKT** (`read_wkt`, `write_wkt`): OGC Well-Known Text, all geometry
//!   types including Z/M/ZM modifiers.
//! - **Binary** (`load_bin`, `load_bin_stream`, `write_bin`): Custom bulk
//!   format for polygons, optimized for batch processing.
//!
//! Optional backends (behind feature flags): Shapefile (`io-shp`), CSV
//! (`io-csv`), GML (`io-gml`), GeoPackage (`io-gpkg`).
//!
//! # Dispatch API
//!
//! High-level functions auto-detect the format by file extension:
//!
//! ```rust,ignore
//! use geo_repair::{diagnose_file, is_valid, load, repair_file, save, MakeValidConfig};
//! let config = MakeValidConfig::default();
//!
//! // Auto-detect: loads .bin or .wkb
//! let geoms = load("input.wkb").unwrap();
//!
//! // Diagnose (like GEOS isValidReason)
//! let results = diagnose_file("input.wkb").unwrap();
//! for (i, r) in results.iter().enumerate() {
//!     if !r.valid {
//!         println!("Polygon {i}: {}", r.reason());
//!     }
//! }
//!
//! // Repair + save
//! repair_file("invalid.wkb", "fixed.wkb", &config).unwrap();
//!
//! // Save geometry
//! save("output.wkb", &geoms[0]).unwrap();
//! ```
//!
//! # Feature gates
//!
//! All I/O requires the `std` feature (enabled by default). In no_std mode,
//! only in-memory WKB parsing is available via `read_wkb` (no file paths).
use std::fs;
use std::io::Read;

use geo::{Coord, Geometry, GeometryCollection, Polygon};

use crate::core::MakeValidConfig;
use crate::validation::{validate, ValidationResult};
use crate::MakeValid;

/// Custom binary format for bulk polygon storage (`.bin`).
pub mod binary;
/// OGC Well-Known Binary (WKB) parsing and serialization.
pub mod wkb;
/// OGC Well-Known Text (WKT) parsing and serialization.
pub mod wkt;

/// Load polygons from a custom binary file.
pub use binary::{load_bin, load_bin_stream, write_bin};
/// Read/write OGC WKB geometry format.
pub use wkb::{estimate_wkb_size, read_wkb, read_wkb_concat, write_wkb};
/// Read/write OGC WKT text format.
pub use wkt::{read_wkt, write_wkt};

// ---------------------------------------------------------------------------
// File-format dispatch by extension
// ---------------------------------------------------------------------------

fn extension(path: &str) -> &str {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
}

/// Load geometries from a file, auto-detecting format by file extension.
///
/// | Extension | Format | Backend | Feature |
/// |-----------|--------|---------|---------|
/// | `.bin` | Custom binary (polygons only) | `binary.rs` | built-in |
/// | `.wkb` / `.wks` | OGC Well-Known Binary | `wkb.rs` | built-in |
/// | `.wkt` | OGC Well-Known Text | `wkt.rs` | built-in |
/// | `.shp` | Shapefile | `shapefile` crate | `io-shp` |
/// | `.csv` | CSV with WKT geometry | `csv` + `wkt` crates | `io-csv` |
/// | `.gml` | GML | `quick-xml` crate | `io-gml` |
/// | `.gpkg` | GeoPackage (SQLite) | `rusqlite` crate | `io-gpkg` |
pub fn load(path: &str) -> Result<Vec<Geometry<f64>>, String> {
    let ext = extension(path).to_lowercase();
    match ext.as_str() {
        "bin" => {
            let polys = load_bin(path)?;
            Ok(polys.into_iter().map(Geometry::Polygon).collect())
        }
        "wkb" | "wks" => {
            let mut buf = Vec::new();
            fs::File::open(path)
                .map_err(|e| format!("cannot open {path}: {e}"))?
                .read_to_end(&mut buf)
                .map_err(|e| format!("cannot read {path}: {e}"))?;
            let geom = read_wkb(&buf).map_err(|e| format!("WKB parse error: {e}"))?;
            Ok(vec![geom])
        }
        "shp" => {
            #[cfg(feature = "io-shp")]
            return load_shp(path);
            #[cfg(not(feature = "io-shp"))]
            Err(
                "'.shp' requires feature 'io-shp': cargo add geo-repair --features io-shp"
                    .to_string(),
            )
        }
        "wkt" => load_wkt(path),
        "csv" => {
            #[cfg(feature = "io-csv")]
            return load_csv(path);
            #[cfg(not(feature = "io-csv"))]
            Err(
                "'.csv' requires feature 'io-csv': cargo add geo-repair --features io-csv"
                    .to_string(),
            )
        }
        "gml" => {
            #[cfg(feature = "io-gml")]
            return load_gml(path);
            #[cfg(not(feature = "io-gml"))]
            Err(
                "'.gml' requires feature 'io-gml': cargo add geo-repair --features io-gml"
                    .to_string(),
            )
        }
        "gpkg" => {
            #[cfg(feature = "io-gpkg")]
            return load_gpkg(path);
            #[cfg(not(feature = "io-gpkg"))]
            Err(
                "'.gpkg' requires feature 'io-gpkg': cargo add geo-repair --features io-gpkg"
                    .to_string(),
            )
        }
        other => Err(format!("unsupported format '.{other}' for '{path}'")),
    }
}

/// Save a geometry to a file, auto-detecting format by file extension.
///
/// | Extension | Format |
/// |-----------|--------|
/// | `.bin` | Custom binary (polygons only) |
/// | `.wkb` / `.wks` | OGC Well-Known Binary |
/// | `.wkt` | OGC Well-Known Text |
pub fn save(path: &str, geom: &Geometry<f64>) -> Result<(), String> {
    let ext = extension(path).to_lowercase();
    match ext.as_str() {
        "bin" => {
            let polys = extract_polygons(geom);
            write_bin(path, &polys)
        }
        "wkb" | "wks" => {
            let bytes = write_wkb(geom);
            fs::write(path, &bytes).map_err(|e| format!("cannot write {path}: {e}"))
        }
        "wkt" => {
            let text = write_wkt(geom);
            fs::write(path, &text).map_err(|e| format!("cannot write {path}: {e}"))
        }
        other => Err(format!(
            "output format '.{other}' not yet supported for '{path}'"
        )),
    }
}

/// Extract all polygons from a geometry for batch binary output.
fn extract_polygons(geom: &Geometry<f64>) -> Vec<Polygon<f64>> {
    match geom {
        Geometry::Polygon(p) => vec![p.clone()],
        Geometry::MultiPolygon(mp) => mp.0.clone(),
        Geometry::GeometryCollection(gc) => gc.0.iter().flat_map(|g| extract_polygons(g)).collect(),
        _ => Vec::new(),
    }
}

/// Load geometries from `input`, repair each one, and save the result to `output`.
///
/// File formats are auto-detected by extension (see [`load`]).
///
/// # Example
///
/// ```rust,no_run
/// use geo_repair::{repair_file, MakeValidConfig, PolyMethod};
/// let cfg = MakeValidConfig { poly_method: PolyMethod::Structure, ..Default::default() };
/// repair_file("invalid.wkb", "fixed.wkb", &cfg).unwrap();
/// ```
pub fn repair_file(input: &str, output: &str, config: &MakeValidConfig) -> Result<(), String> {
    let geoms = load(input)?;
    let mut results = Vec::with_capacity(geoms.len());
    for g in &geoms {
        results.push(g.make_valid_with_config(config));
    }
    let result = if results.len() == 1 {
        results.into_iter().next().unwrap()
    } else {
        Geometry::GeometryCollection(GeometryCollection(results))
    };
    save(output, &result)
}

/// Load geometries from `path` and validate each one, returning per-geometry
/// results (mirrors GEOS `isValid` / `isValidReason`).
///
/// File format is auto-detected by extension (see [`load`]).
///
/// # Example
///
/// ```rust,no_run
/// use geo_repair::diagnose_file;
/// let results = diagnose_file("input.bin").unwrap();
/// for (i, r) in results.iter().enumerate() {
///     println!("Geometry {i}: {} — {}", if r.valid { "valid" } else { "INVALID" }, r.reason());
/// }
/// ```
pub fn diagnose_file(path: &str) -> Result<Vec<ValidationResult>, String> {
    let geoms = load(path)?;
    Ok(geoms.iter().map(validate).collect())
}

// ---------------------------------------------------------------------------
// Feature-gated backend stubs (implemented when feature is enabled)
// ---------------------------------------------------------------------------

#[cfg(feature = "io-shp")]
fn load_shp(_path: &str) -> Result<Vec<Geometry<f64>>, String> {
    Err("Shapefile backend not yet implemented".into())
}

fn load_wkt(path: &str) -> Result<Vec<Geometry<f64>>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let geom = read_wkt(&text)?;
    Ok(vec![geom])
}

#[cfg(feature = "io-csv")]
fn load_csv(_path: &str) -> Result<Vec<Geometry<f64>>, String> {
    Err("CSV backend not yet implemented".into())
}

#[cfg(feature = "io-gml")]
fn load_gml(_path: &str) -> Result<Vec<Geometry<f64>>, String> {
    Err("GML backend not yet implemented".into())
}

#[cfg(feature = "io-gpkg")]
fn load_gpkg(_path: &str) -> Result<Vec<Geometry<f64>>, String> {
    Err("GeoPackage backend not yet implemented".into())
}

// ---------------------------------------------------------------------------
// Geometry area utilities (used by tests and benchmarks)
// ---------------------------------------------------------------------------

/// Compute the signed area of a polygon ring using the shoelace formula.
/// Positive values indicate CCW winding, negative values CW winding.
pub fn signed_area(ring: &[Coord<f64>]) -> f64 {
    let mut s = 0.0;
    for w in ring.windows(2) {
        s += w[0].x * w[1].y - w[1].x * w[0].y;
    }
    s / 2.0
}

/// Compute the absolute area of a polygon from its exterior ring.
pub fn polygon_area(p: &Polygon<f64>) -> f64 {
    signed_area(&p.exterior().0).abs()
}

/// Compute the total area of a polygon or multipolygon geometry.
/// Returns 0.0 for non-polygon types.
pub fn geo_area(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::Polygon(p) => polygon_area(p),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(polygon_area).sum(),
        _ => 0.0,
    }
}

/// Count the number of sub-polygons in a geometry.
/// Returns 1 for `Polygon`, `n` for `MultiPolygon`, 0 for other types.
pub fn count_sub_polys(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::Polygon(_) => 1,
        Geometry::MultiPolygon(mp) => mp.0.len(),
        _ => 0,
    }
}

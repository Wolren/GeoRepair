//! Shared test assertion helpers for robust, OGC-compliant testing.
//!
//! All test files should use `#[path = "../common/mod.rs"] mod common;`
//! and then call `common::*` or `use common::*`.

use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::validation::{GeoValidation, GeometryValidationError};
use geo_repair::{MakeValidConfig, PolyMethod};

/// GEOS IsValidOp parity for the line family (verified against the GEOS XML
/// corpus and geosop): a LineString is valid iff coordinates are finite and
/// the component has >= 2 DISTINCT points. Simplicity (NotSimple) does NOT
/// invalidate a line - OGC/PostGIS explicitly allow non-simple open curves
/// as valid. Zero-length lines (1 distinct point) are invalid.
pub fn line_family_geos_valid(g: &Geometry<f64>) -> bool {
    fn ls_ok(ls: &LineString<f64>) -> bool {
        if ls.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
            return false;
        }
        let mut prev: Option<Coord<f64>> = None;
        let mut distinct = 0usize;
        for &c in ls.0.iter() {
            if prev != Some(c) {
                distinct += 1;
                prev = Some(c);
            }
        }
        distinct >= 2
    }
    match g {
        Geometry::Line(l) => {
            l.start.x.is_finite()
                && l.start.y.is_finite()
                && l.end.x.is_finite()
                && l.end.y.is_finite()
                && l.start != l.end
        }
        Geometry::LineString(ls) => ls_ok(ls),
        Geometry::MultiLineString(mls) => mls.0.iter().all(ls_ok),
        _ => true,
    }
}

/// Assert the geometry is OGC-valid (using our Shewchuk-based validator).
/// For the line family the OGC/GEOS rule is applied: NotSimple does not
/// invalidate (our validator is stricter and flags it).
pub fn assert_valid(g: &Geometry<f64>) {
    let r = g.validate();
    if matches!(
        g,
        Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_)
    ) {
        let ok = line_family_geos_valid(g);
        assert!(ok, "expected valid, got: {:?}", r.errors);
        return;
    }
    assert!(r.valid, "expected valid, got: {:?}", r.errors);
}

/// True if the only validator errors are NotSimple on line-family output.
#[allow(dead_code)]
pub fn non_simple_only(g: &Geometry<f64>) -> bool {
    let r = g.validate();
    !r.errors.is_empty()
        && r.errors
            .iter()
            .all(|e| matches!(e, GeometryValidationError::NotSimple))
}

/// Assert the geometry is OGC-valid AND all polygon rings
/// follow OGC orientation (exterior CCW, interiors CW).
pub fn assert_valid_ogc(g: &Geometry<f64>) {
    assert_valid(g);
    assert_ogc_oriented(g);
}

/// Assert the geometry is non-empty (not an empty collection).
pub fn assert_not_empty(g: &Geometry<f64>) {
    let empty = match g {
        Geometry::GeometryCollection(gc) => gc.0.is_empty(),
        Geometry::MultiPoint(mp) => mp.0.is_empty(),
        Geometry::MultiLineString(mls) => mls.0.is_empty(),
        Geometry::MultiPolygon(mp) => mp.0.is_empty(),
        _ => false,
    };
    assert!(!empty, "expected non-empty geometry, got: {g:?}");
}

/// Count the number of top-level geometric components in a Geometry.
#[allow(dead_code)]
pub fn count_geometries(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::Point(_) => 1,
        Geometry::LineString(_) => 1,
        Geometry::Polygon(_) => 1,
        Geometry::MultiPoint(mp) => mp.0.len(),
        Geometry::MultiLineString(mls) => mls.0.len(),
        Geometry::MultiPolygon(mpoly) => mpoly.0.len(),
        Geometry::GeometryCollection(gc) => gc.0.len(),
        _ => 0,
    }
}

/// Assert the result is a MultiPolygon with exactly `expected` component polygons.
#[allow(dead_code)]
pub fn assert_multipolygon_count(g: &Geometry<f64>, expected: usize) {
    let msg = format!("expected MultiPolygon with {expected} polygon(s), got: {g:?}");
    match g {
        Geometry::MultiPolygon(mp) => assert_eq!(mp.0.len(), expected, "{msg}"),
        other => panic!("{msg} (got {})", geometry_type_name(other)),
    }
}

/// Assert the result is exactly a Polygon (not MultiPolygon, not GC).
/// Accepts MultiPolygon with a single polygon as equivalent.
#[allow(dead_code)]
pub fn assert_is_polygon(g: &Geometry<f64>) {
    let ok = matches!(g, Geometry::Polygon(_))
        || matches!(g, Geometry::MultiPolygon(mp) if mp.0.len() == 1);
    assert!(ok, "expected Polygon (or MultiPolygon[1]), got: {g:?}");
}

/// Assert the result is a Polygon with no holes.
#[allow(dead_code)]
pub fn assert_simple_polygon(g: &Geometry<f64>) {
    match g {
        Geometry::Polygon(p) => {
            assert!(
                p.interiors().is_empty(),
                "expected Polygon without holes, got: {g:?}"
            );
        }
        other => panic!("expected Polygon, got: {}", geometry_type_name(other)),
    }
}

/// Assert that Polygon rings follow OGC orientation:
/// exterior CCW, interior rings (holes) CW.
pub fn assert_ogc_oriented(g: &Geometry<f64>) {
    match g {
        Geometry::Polygon(poly) => assert_polygon_orientation(poly),
        Geometry::MultiPolygon(mp) => {
            for poly in mp.iter() {
                assert_polygon_orientation(poly);
            }
        }
        _ => {}
    }
}

fn ring_signed_area(coords: &[Coord<f64>]) -> f64 {
    let mut area = 0.0;
    for i in 0..coords.len().saturating_sub(1) {
        area += coords[i].x * coords[i + 1].y - coords[i + 1].x * coords[i].y;
    }
    area
}

fn is_ccw(ring: &LineString<f64>) -> bool {
    ring_signed_area(&ring.0) > 0.0
}

fn is_cw(ring: &LineString<f64>) -> bool {
    ring_signed_area(&ring.0) < 0.0
}

fn assert_polygon_orientation(poly: &Polygon<f64>) {
    let ext = poly.exterior();
    assert!(
        is_ccw(ext) || ext.0.len() < 4,
        "OGC: exterior ring must be CCW, got CW: {:?}",
        ext
    );
    for (i, ring) in poly.interiors().iter().enumerate() {
        assert!(
            is_cw(ring) || ring.0.len() < 4,
            "OGC: interior ring {i} must be CW, got CCW: {:?}",
            ring
        );
    }
}

/// Assert the result is an empty GeometryCollection or other empty multi-type.
pub fn assert_is_empty(g: &Geometry<f64>) {
    let empty = match g {
        Geometry::GeometryCollection(gc) => gc.0.is_empty(),
        Geometry::MultiPoint(mp) => mp.0.is_empty(),
        Geometry::MultiLineString(mls) => mls.0.is_empty(),
        Geometry::MultiPolygon(mp) => mp.0.is_empty(),
        Geometry::Point(p) => !p.x().is_finite() || !p.y().is_finite(),
        _ => false,
    };
    assert!(empty, "expected empty geometry, got: {g:?}");
}

/// Assert the output equals the input (valid geometry should pass through unchanged).
#[allow(dead_code)]
pub fn assert_unchanged(input: &Geometry<f64>, output: &Geometry<f64>) {
    assert_eq!(
        input, output,
        "expected unchanged geometry, but output differs from input"
    );
}

/// Returns a human-readable geometry type name.
#[allow(dead_code)]
pub fn geometry_type_name(g: &Geometry<f64>) -> &'static str {
    match g {
        Geometry::Point(_) => "Point",
        Geometry::LineString(_) => "LineString",
        Geometry::Polygon(_) => "Polygon",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GeometryCollection",
        _ => "Unknown",
    }
}

/// Create an Auto config.
pub fn cfg_auto() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Auto,
        ..Default::default()
    }
}

/// Create an Arrange config.
#[allow(dead_code)]
pub fn cfg_arrange() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    }
}

/// Create a Structure config.
#[allow(dead_code)]
pub fn cfg_structure() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    }
}

/// Parse a WKT string into a Geometry (our own reader).
pub fn geom_from_wkt(s: &str) -> Geometry<f64> {
    geo_repair::io::wkt::read_wkt(s).unwrap()
}

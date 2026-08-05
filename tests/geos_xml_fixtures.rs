use geo::{Coord, Geometry, LineString, Point, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::MakeValid;

#[path = "common/mod.rs"]
mod common;
use common::*;

// =========================================================================
// GEOS XML MakeValid test fixtures
// Ported from: tests/xmltester/tests/misc/makevalid.xml
// =========================================================================

// ---------------------------------------------------------------------------
// 1. point/already_valid
// ---------------------------------------------------------------------------
// Input: POINT(0 0), Expected: POINT(0 0)
#[test]
fn xml_point_valid() {
    let g = geom_from_wkt("POINT (0 0)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_eq!(result, g, "valid point should return unchanged");
}

// ---------------------------------------------------------------------------
// 2. point/empty
// ---------------------------------------------------------------------------
// Input: POINT EMPTY, Expected: POINT EMPTY
#[test]
fn xml_point_empty() {
    let g = geom_from_wkt("POINT EMPTY");
    let result = g.make_valid_with_config(&cfg_auto());
    // POINT EMPTY → empty GeometryCollection — correct behavior
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// 3. linestring/already_valid
// ---------------------------------------------------------------------------
// Input: LINESTRING(0 0, 1 1), Expected: LINESTRING(0 0, 1 1)
#[test]
fn xml_linestring_valid() {
    let g = geom_from_wkt("LINESTRING (0 0, 1 1)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_eq!(result, g, "valid linestring should return unchanged");
}

// ---------------------------------------------------------------------------
// 4. linestring/invalid_result_point (collapsed to point)
// ---------------------------------------------------------------------------
// Input: LINESTRING(0 0, 0 0), Expected: POINT(0 0)
#[test]
fn xml_linestring_collapsed_to_point() {
    let g = geom_from_wkt("LINESTRING (0 0, 0 0)");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_eq!(
        result,
        Geometry::Point(Point::new(0.0, 0.0)),
        "collapsed linestring should preserve Point (GEOS compat)"
    );
}

// ---------------------------------------------------------------------------
// 5. linestring/empty
// ---------------------------------------------------------------------------
#[test]
fn xml_linestring_empty() {
    let g = geom_from_wkt("LINESTRING EMPTY");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// 6. multilinestring/empty
// ---------------------------------------------------------------------------
#[test]
fn xml_multilinestring_empty() {
    let g = geom_from_wkt("MULTILINESTRING EMPTY");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// ---------------------------------------------------------------------------
// 7. multilinestring/case1: collapsed + valid components -> GC
// ---------------------------------------------------------------------------
// Input: MULTILINESTRING((0 0,0 0),(1 1,2 2))
// Expected: GEOMETRYCOLLECTION(LINESTRING(1 1,2 2), POINT(0 0))
#[test]
fn xml_multilinestring_case1() {
    let g = geom_from_wkt("MULTILINESTRING ((0 0, 0 0), (1 1, 2 2))");
    let result = g.make_valid_with_config(&cfg_auto());
    // GEOS exact output (makevalid.xml) - asserted, not just typed:
    let expected = geom_from_wkt("GEOMETRYCOLLECTION (LINESTRING (1 1, 2 2), POINT (0 0))");
    assert_eq!(
        result, expected,
        "GEOS makevalid.xml case 7 exact output"
    );
}

/// Flatten a geometry to (line coordinate lists, point coordinates) so a
/// GEOS GC grouping (MULTILINESTRING instead of separate LINESTRINGs,
/// component order) does not hide a real content difference.
fn flatten_gc(g: &Geometry<f64>) -> (Vec<Vec<(f64, f64)>>, Vec<(f64, f64)>) {
    let mut lines = Vec::new();
    let mut points = Vec::new();
    fn walk(
        g: &Geometry<f64>,
        lines: &mut Vec<Vec<(f64, f64)>>,
        points: &mut Vec<(f64, f64)>,
    ) {
        match g {
            Geometry::LineString(ls) => {
                lines.push(ls.0.iter().map(|c| (c.x, c.y)).collect());
            }
            Geometry::MultiLineString(mls) => {
                for ls in &mls.0 {
                    lines.push(ls.0.iter().map(|c| (c.x, c.y)).collect());
                }
            }
            Geometry::Point(p) => points.push((p.x(), p.y())),
            Geometry::MultiPoint(mp) => {
                for p in &mp.0 {
                    points.push((p.x(), p.y()));
                }
            }
            Geometry::GeometryCollection(gc) => {
                for sub in &gc.0 {
                    walk(sub, lines, points);
                }
            }
            _ => {}
        }
    }
    walk(g, &mut lines, &mut points);
    (lines, points)
}

// ---------------------------------------------------------------------------
// 8. multilinestring/case2: two valid lines -> MLS (GEOS groups into MLS;
// we emit separate LINESTRINGs - assert component-set parity)
// ---------------------------------------------------------------------------
// Input: MULTILINESTRING((0 0,0 0),(1 1,2 2),(2 2,3 3))
// Expected: GEOMETRYCOLLECTION(MULTILINESTRING((2 2,3 3),(1 1,2 2)), POINT(0 0))
#[test]
fn xml_multilinestring_case2() {
    let g = geom_from_wkt("MULTILINESTRING ((0 0, 0 0), (1 1, 2 2), (2 2, 3 3))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    let (lines, points) = flatten_gc(&result);
    // GEOS expected: two lines (1 1,2 2) and (2 2,3 3), one point (0 0)
    assert_eq!(
        lines,
        vec![
            vec![(1.0, 1.0), (2.0, 2.0)],
            vec![(2.0, 2.0), (3.0, 3.0)]
        ],
        "GEOS makevalid.xml case 8 line components"
    );
    assert_eq!(points, vec![(0.0, 0.0)], "GEOS makevalid.xml case 8 point component");
}

// ---------------------------------------------------------------------------
// 9. multilinestring/case2 (dup name): two collapses -> MultiPoint
// ---------------------------------------------------------------------------
// Input: MULTILINESTRING((0 0,0 0),(1 1,2 2),(2 2,3 3),(4 4,4 4))
// Expected: GEOMETRYCOLLECTION(MULTILINESTRING((2 2,3 3),(1 1,2 2)), MULTIPOINT(4 4,0 0))
#[test]
fn xml_multilinestring_two_collapses() {
    let g = geom_from_wkt("MULTILINESTRING ((0 0, 0 0), (1 1, 2 2), (2 2, 3 3), (4 4, 4 4))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    let (lines, points) = flatten_gc(&result);
    assert_eq!(
        lines,
        vec![
            vec![(1.0, 1.0), (2.0, 2.0)],
            vec![(2.0, 2.0), (3.0, 3.0)]
        ],
        "GEOS makevalid.xml case 9 line components"
    );
    let mut pts = points.clone();
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(
        pts,
        vec![(0.0, 0.0), (4.0, 4.0)],
        "GEOS makevalid.xml case 9 point components"
    );
}

/// Relative-quantized ring fingerprint: points sorted, closure-insensitive,
/// quantized to 1e-9 RELATIVE precision (absorbs legitimate fp noise from
/// boolean-op intersection points - measured: we emit 0.5 0.09999999962747
/// where GEOS keeps 0.5 0.1). Same normalization geos_compare.rs uses.
fn ring_fp(ring: &[Coord<f64>]) -> Vec<(i64, i64)> {
    let mag = ring
        .iter()
        .map(|c| c.x.abs().max(c.y.abs()))
        .fold(0.0_f64, f64::max);
    let mag = if mag == 0.0 { 1.0 } else { mag };
    let q = |v: f64| ((v / mag) * 1e9).round() as i64;
    let mut pts: Vec<(i64, i64)> = ring.iter().map(|c| (q(c.x), q(c.y))).collect();
    if pts.first() == pts.last() {
        pts.pop();
    }
    pts.sort_unstable();
    pts
}

/// Polygon fingerprint: (shell fp, sorted hole fps).
fn poly_fp(p: &Polygon<f64>) -> (Vec<(i64, i64)>, Vec<Vec<(i64, i64)>>) {
    let ext = ring_fp(&p.exterior().0);
    let mut holes: Vec<Vec<(i64, i64)>> = p
        .interiors()
        .iter()
        .map(|h| ring_fp(&h.0))
        .collect();
    holes.sort_unstable();
    (ext, holes)
}

/// All polygon fingerprints of a geometry (Polygon / MultiPolygon /
/// GeometryCollection-recursing), sorted - a winding/rotation/order-
/// insensitive shape signature.
fn poly_fp_set(g: &Geometry<f64>) -> Vec<(Vec<(i64, i64)>, Vec<Vec<(i64, i64)>>)> {
    let mut acc = Vec::new();
    match g {
        Geometry::Polygon(p) => acc.push(poly_fp(p)),
        Geometry::MultiPolygon(mp) => {
            for p in &mp.0 {
                acc.push(poly_fp(p));
            }
        }
        Geometry::GeometryCollection(gc) => {
            for c in &gc.0 {
                acc.extend(poly_fp_set(c));
            }
        }
        _ => {}
    }
    acc.sort_unstable();
    acc
}

/// Total unsigned polygon area (Polygon / MultiPolygon / GC-recursing).
fn total_poly_area(g: &Geometry<f64>) -> f64 {
    use geo::Area;
    match g {
        Geometry::Polygon(p) => p.unsigned_area(),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(|p| p.unsigned_area()).sum(),
        Geometry::GeometryCollection(gc) => gc.0.iter().map(total_poly_area).sum(),
        _ => 0.0,
    }
}

fn component_count(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::MultiPolygon(mp) => mp.0.len(),
        Geometry::MultiLineString(mls) => mls.0.len(),
        Geometry::MultiPoint(mp) => mp.0.len(),
        Geometry::GeometryCollection(gc) => gc.0.len(),
        _ => 1,
    }
}

/// Assert shape parity with a GEOS expected WKT: same polygon fingerprint
/// set (winding/rotation/order-insensitive) AND same total polygon area.
/// This is the strongest comparison that still tolerates our deliberate
/// OGC re-orientation (GEOS preserves input winding; we enforce it).
fn assert_shape_parity(actual: &Geometry<f64>, expected_wkt: &str) {
    let expected = geom_from_wkt(expected_wkt);
    let a_fp = poly_fp_set(actual);
    let e_fp = poly_fp_set(&expected);
    assert_eq!(
        a_fp, e_fp,
        "shape fingerprint mismatch:\nactual:   {actual:?}\nexpected: {expected:?}"
    );
    let a_area = total_poly_area(actual);
    let e_area = total_poly_area(&expected);
    let scale = e_area.abs().max(1.0);
    assert!(
        (a_area - e_area).abs() <= 1e-6 * scale,
        "area mismatch: actual {a_area}, expected {e_area}"
    );
}

/// True when the geometry contains an empty POINT component (GEOS keeps
/// POINT EMPTY in GC makeValid outputs; we keep it too but drop other
/// empties).
fn has_empty_point(g: &Geometry<f64>) -> bool {
    match g {
        Geometry::Point(p) => !p.0.x.is_finite() || !p.0.y.is_finite(),
        Geometry::GeometryCollection(gc) => gc.0.iter().any(has_empty_point),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 10. polygon/already_valid
// ---------------------------------------------------------------------------
#[test]
fn xml_polygon_valid() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 1, 1 1, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    // GEOS exact output (makevalid.xml case 10): the same triangle. GEOS
    // preserves the CW ring; we enforce OGC orientation - fingerprint
    // parity asserts the shape modulo winding/rotation.
    assert_shape_parity(&result, "POLYGON ((0 0, 0 1, 1 1, 0 0))");
}

// ---------------------------------------------------------------------------
// 11. polygon/crossing (bowtie)
// ---------------------------------------------------------------------------
#[test]
fn xml_polygon_bowtie() {
    let g = geom_from_wkt("POLYGON ((0 0, 1 1, 0 1, 1 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    // GEOS exact output (makevalid.xml case 11): the two bowtie lobes.
    assert_shape_parity(
        &result,
        "MULTIPOLYGON (((0.0 1.0, 1.0 1.0, 0.5 0.5, 0.0 1.0)), ((0.0 0.0, 0.5 0.5, 1.0 0.0, 0.0 0.0)))",
    );
}

// ---------------------------------------------------------------------------
// 12. polygon/hole_touching_two_places
// ---------------------------------------------------------------------------
#[test]
fn xml_hole_touching_two_places() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 1, 1 1, 1 0, 0 0), (0 0.5, 0.5 0.1, 1 0.5, 0 0.5))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    // GEOS exact output (makevalid.xml case 12): the two components split
    // by the hole touching the shell at two points. Fingerprint parity
    // absorbs the 0.5 0.09999999962747 / 0.5 0.1 fp noise.
    assert_shape_parity(
        &result,
        "MULTIPOLYGON (((0.0 0.0, 0.0 0.5, 0.5 0.1, 1.0 0.5, 1.0 0.0, 0.0 0.0)), ((0.0 0.5, 0.0 1.0, 1.0 1.0, 1.0 0.5, 0.0 0.5)))",
    );
}

// ---------------------------------------------------------------------------
// 13. multipolygon/second_part_overlapping
// ---------------------------------------------------------------------------
#[test]
fn xml_multipolygon_overlapping() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (((0 0, 0 1, 1 1, 1 0, 0 0)), ((0.8 0.1, 2 0.1, 2 0.9, 0.8 0.9, 0.8 0.1)))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    // PINNED divergence (makevalid.xml case 13, XML comment "not completely
    // sure"): GEOS emits the even-odd notch split (area 1.64); we union
    // overlapping shells per JTS rule 09 (area 1.8 = exact set-theoretic
    // union of the 1x1 and (0.8..2)x(0.1..0.9) rects: 1 + 0.96 - 0.16).
    // Mirrors the geos_compare.rs mp_overlapping pin (46.0).
    let area = total_poly_area(&result);
    let scale = area.abs().max(1.0);
    assert!(
        (area - 1.8).abs() <= 1e-6 * scale,
        "pinned union answer moved: got {area}, expected 1.8 (GEOS even-odd: 1.64)"
    );
    assert_eq!(
        component_count(&result),
        1,
        "union of overlapping shells must collapse to one component"
    );
}

// ---------------------------------------------------------------------------
// 14. multipolygon/first_part_crossing_second_part_overlapping
// ---------------------------------------------------------------------------
#[test]
fn xml_multipolygon_crossing_overlapping() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (((0 0, 1 1, 0 1, 1 0, 0 0)), ((0.8 0.1, 2 0.1, 2 0.9, 0.8 0.9, 0.8 0.1)))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    // PINNED divergence (makevalid.xml case 14), measured three ways
    // (2026-08-05): XML expected WKT area 1.445; GEOS makeValid TODAY emits
    // 1.44 (the XML expectation is stale - GEOS itself moved); we emit 1.45,
    // which is the EXACT set-theoretic union: bowtie lobes 0.25 + 0.25 +
    // rect 0.96, minus the two 0.005 overlap wedges (lobe1 x 0.8..0.9 and
    // lobe2 y 0.8..0.9) = 1.45. Our union semantics are pinned; GEOS's
    // even-odd numbers are documented, not asserted.
    let area = total_poly_area(&result);
    let scale = area.abs().max(1.0);
    assert!(
        (area - 1.45).abs() <= 1e-6 * scale,
        "pinned union answer moved: got {area}, expected 1.45 (GEOS-now 1.44, XML 1.445)"
    );
    assert_eq!(
        component_count(&result),
        1,
        "union of crossing+overlapping shells must collapse to one component"
    );
}

// ---------------------------------------------------------------------------
// 15. geometry_collection with invalid polygon + empty sub-geometries
// ---------------------------------------------------------------------------
#[test]
fn xml_geometry_collection_with_empties() {
    let g = geom_from_wkt(
        "GEOMETRYCOLLECTION (POINT EMPTY, LINESTRING EMPTY, POLYGON ((0 0, 0 1, 1 1, 1 0, 0 0), (0 0.5, 0.5 0.1, 1 0.5, 0 0.5)))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    // GEOS expected (makevalid.xml case 15): GC(MULTIPOLYGON(two components
    // as in case 12), LINESTRING EMPTY, POINT EMPTY). We keep POINT EMPTY
    // (as POINT(NaN NaN)) and drop LINESTRING EMPTY - assert the polygon
    // components by fingerprint parity plus the retained empty point.
    assert_shape_parity(
        &result,
        "MULTIPOLYGON (((0.0 0.0, 0.0 0.5, 0.5 0.1, 1.0 0.5, 1.0 0.0, 0.0 0.0)), ((0.0 0.5, 0.0 1.0, 1.0 1.0, 1.0 0.5, 0.0 0.5)))",
    );
    assert!(
        has_empty_point(&result),
        "GEOS keeps POINT EMPTY in GC makeValid output (LINESTRING EMPTY is dropped), got: {result:?}"
    );
}

// =========================================================================
// GEOS unit-test port: operation/valid/MakeValidTest.cpp test<1>
// (https://github.com/libgeos/geos/issues/265 - the 12-vertex polygon).
// GEOS asserts: input invalid, output valid.
// Measured parity (2026-08-05, geosop 3.14.1): GEOS makeValid area
// 34.749988580488314, ours 34.749988575662314 (diff 4.9e-9 absolute,
// 1.4e-10 relative) - the SAME even-odd repair, including the same hole.
// NOTE: the input's unary_union area (36.3265) is NOT the right anchor
// here: the self-crossing ring's lobes overlap, and even-odd semantics
// exclude double-covered regions (union counts them). GEOS and we agree
// on the even-odd answer; the pin below is that shared answer.
// =========================================================================
#[test]
fn geos_makevalid_test1_issue265_polygon() {
    use geo::Area;

    let g = geom_from_wkt("POLYGON ((2.22 2.28, 7.67 2.06, 10.98 7.70, 9.39 5.00, 7.96 7.12, 6.77 5.16, 7.43 6.24, 3.70 7.22, 5.72 5.77, 4.18 10.74, 2.20 6.83, 2.22 2.28))");

    // GEOS asserts !isValid() on the input.
    assert!(!g.validate().valid, "issue-265 input must be invalid (GEOS parity)");

    let result = g.make_valid_with_config(&cfg_auto());
    // GEOS asserts isValid() on the output.
    assert_valid_ogc(&result);
    assert_not_empty(&result);

    // Repair is idempotent (GEOS contract: makeValid(makeValid(x)) == makeValid(x)).
    let result2 = result.make_valid_with_config(&cfg_auto());
    assert_eq!(result, result2, "issue-265 repair must be idempotent");

    // Area parity with GEOS's measured answer (even-odd semantics, see
    // comment above). If the repair semantics ever change, this pin flips
    // the behavior change into a visible failure.
    let out_area = match &result {
        Geometry::Polygon(p) => p.unsigned_area(),
        other => panic!("issue-265 expected polygon output, got {other:?}"),
    };
    let geos_area = 34.749988580488314_f64;
    let scale = geos_area.abs().max(1.0);
    assert!(
        (out_area - geos_area).abs() <= 1e-6 * scale,
        "issue-265 repair must match GEOS even-odd area (ours {out_area}, GEOS {geos_area})"
    );
}

// =========================================================================
// GEOS unit-test port: operation/valid/MakeValidTest.cpp test<4>
// (https://github.com/libgeos/geos/issues/265, WKB from PostGIS
// liblwgeom/cunit/cu_geos.c#L147)
// =========================================================================
#[test]
fn geos_makevalid_test4_postgis_ring() {
    use geo::Area;

    let hex = concat!(
        "0103000000010000000900000062105839207df640378941e09d491c41ced67431387df640c667e7d398491",
        "c4179e92631387df640d9cef7d398491c41fa7e6abcf87df640cdcccc4c70491c41e3a59bc4527df64052b8",
        "1e053f491c41cdcccccc5a7ef640e3a59bc407491c4104560e2da27df640aaf1d24dd3481c41e9263108c67",
        "bf64048e17a1437491c4162105839207df640378941e09d491c41",
    );
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let g = geo_repair::io::wkb::read_wkb(&bytes).expect("GEOS test<4> WKB parses");

    // GEOS asserts !isValid() on the input.
    assert!(!g.validate().valid, "test<4> input must be invalid (GEOS parity)");

    let result = g.make_valid_with_config(&cfg_auto());
    // GEOS asserts isValid() on the output.
    assert_valid_ogc(&result);

    // GEOS expected exact output (WKTWriter trim):
    // POLYGON((92127.546 463452.075,92117.173 463439.755,92133.675 463425.942,
    //          92122.136 463412.826,92092.377 463437.77,92114.014 463463.469,
    //          92115.512 463462.207,92115.51207431706 463462.2069374289,
    //          92127.546 463452.075))
    // Divergence (documented, not asserted bit-exact): GEOS keeps both the
    // spike vertex (92115.512) and the noded intersection (92115.51207431706)
    // as separate vertices (9-vertex ring); our repair merges them into one
    // (8-vertex ring) and the merged coordinates differ from GEOS's by
    // ~1e-9 absolute. The parity properties that DO hold are asserted:
    // valid output and area preservation.
    let input_area = match &g {
        Geometry::Polygon(p) => p.unsigned_area(),
        _ => unreachable!(),
    };
    let out_area = match &result {
        Geometry::Polygon(p) => p.unsigned_area(),
        other => panic!("test<4> expected polygon output, got {other:?}"),
    };
    let scale = input_area.abs().max(1.0);
    assert!(
        (out_area - input_area).abs() <= 1e-6 * scale,
        "test<4> repair must preserve area (input {input_area}, output {out_area})"
    );
}

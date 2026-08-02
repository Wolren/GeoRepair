//! Ports of GEOS unit test suites that the XML corpus does not reach:
//! - tests/unit/operation/valid/RepeatedPointRemoverTest.cpp (17 cases)
//! - tests/unit/operation/valid/ValidClosedRingTest.cpp (5 cases)
//!
//! Source-verified against the GEOS clone at /d/Projects/gis/lib/geos
//! (commit cad26ad98 "Return EMPTY components when repeated point removal
//! renders the underlying parts invalid").
//!
//! GEOS's `ensure_equals_geometry` normalizes both sides before comparing
//! (tests/unit/utility.h), so ring rotation is irrelevant to the assertion.
//! We mirror that: rings are compared as cyclic rotations. All expected
//! coordinate values are exact (the remover only drops points, never moves
//! them), so exact f64 equality holds.

use geo::{Coord, Geometry, Polygon};
use geo_repair::MakeValid;
use geo_repair::validation::{GeometryValidationError, validate};
use geo_repair::{remove_repeated_coords, remove_repeated_points};

#[path = "common/mod.rs"]
mod common;
use common::*;

// =========================================================================
// RepeatedPointRemoverTest (17 cases; 6 skipped: Z/M dimensions, we are 2D)
// =========================================================================

/// Parse a line WKT and return its coordinates.
fn line_coords(wkt: &str) -> Vec<Coord<f64>> {
    match geom_from_wkt(wkt) {
        Geometry::LineString(ls) => ls.0,
        other => panic!("expected linestring, got {other:?}"),
    }
}

/// Sequence-level expected equality.
fn assert_seq_eq(input: &str, expected: &str, tolerance: f64) {
    let out = remove_repeated_coords(&line_coords(input), tolerance);
    let exp = line_coords(expected);
    assert_eq!(out, exp, "sequence filter mismatch");
}

/// Ring equality under cyclic rotation (GEOS normalize semantics). Closed
/// rings carry a duplicated closing coordinate; the distinct cycle is
/// compared, exactly like GEOS normalize + equalsExact does.
fn ring_rot_eq(a: &[Coord<f64>], b: &[Coord<f64>]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if a.is_empty() {
        return true;
    }
    let a_cycle: &[Coord<f64>] = if a.first() == a.last() {
        &a[..a.len() - 1]
    } else {
        a
    };
    let b_cycle: &[Coord<f64>] = if b.first() == b.last() {
        &b[..b.len() - 1]
    } else {
        b
    };
    if a_cycle.len() != b_cycle.len() {
        return false;
    }
    (0..a_cycle.len())
        .any(|k| (0..a_cycle.len()).all(|i| a_cycle[(k + i) % a_cycle.len()] == b_cycle[i]))
}

/// Geometry-level expected equality: lines exact, rings cyclic.
fn assert_geom_eq(input: &str, expected: &str, tolerance: f64) {
    let out = remove_repeated_points(&geom_from_wkt(input), tolerance);
    let exp = geom_from_wkt(expected);
    match (&out, &exp) {
        (Geometry::LineString(a), Geometry::LineString(b)) => {
            assert_eq!(a.0, b.0, "line mismatch");
        }
        (Geometry::Polygon(a), Geometry::Polygon(b)) => {
            assert!(
                ring_rot_eq(&a.exterior().0, &b.exterior().0),
                "shell mismatch: {:?} vs {:?}",
                a.exterior().0,
                b.exterior().0
            );
            assert_eq!(a.interiors().len(), b.interiors().len(), "hole count");
            for (ha, hb) in a.interiors().iter().zip(b.interiors()) {
                assert!(ring_rot_eq(&ha.0, &hb.0), "hole mismatch");
            }
        }
        (Geometry::MultiPolygon(a), Geometry::MultiPolygon(b)) => {
            assert_eq!(a.0.len(), b.0.len(), "part count");
            for (pa, pb) in a.0.iter().zip(&b.0) {
                assert!(
                    ring_rot_eq(&pa.exterior().0, &pb.exterior().0),
                    "part shell mismatch"
                );
                assert_eq!(pa.interiors().len(), pb.interiors().len(), "part hole count");
                for (ha, hb) in pa.interiors().iter().zip(pb.interiors()) {
                    assert!(ring_rot_eq(&ha.0, &hb.0), "part hole mismatch");
                }
            }
        }
        _ => panic!("unexpected geometry pair: {out:?} vs {exp:?}"),
    }
}

#[test]
fn rpr_test_1_exact_dups_removed() {
    // "(3 7, 8 8, 8 8, 8 8, 10 9)" -> "(3 7, 8 8, 10 9)"
    assert_seq_eq("LINESTRING (3 7, 8 8, 8 8, 8 8, 10 9)", "LINESTRING (3 7, 8 8, 10 9)", 0.0);
}

#[test]
fn rpr_test_2_exact_dups_tail() {
    // "(3 7, 8 8, 8 8, 8 8)" -> "(3 7, 8 8)"
    assert_seq_eq("LINESTRING (3 7, 8 8, 8 8, 8 8)", "LINESTRING (3 7, 8 8)", 0.0);
}

#[test]
fn rpr_test_3_sequence_within_tolerance() {
    // CoordinateSequences just retain each coordinate within the tolerance.
    assert_seq_eq("LINESTRING (0 0, 1 0, 4 0, 5 0)", "LINESTRING (0 0, 4 0)", 3.0);
}

#[test]
fn rpr_test_4_line_keeps_last_point() {
    // Linestrings note the last point and retain it in preference over the
    // internal point.
    assert_geom_eq("LINESTRING (0 0, 1 0, 4 0, 5 0)", "LINESTRING (0 0, 5 0)", 3.0);
}

#[test]
fn rpr_test_5_polygon_shell_filtered() {
    assert_geom_eq(
        "MULTIPOLYGON (((0 0, 9 0, 10 0, 10 10, 0 10, 0 1, 0 0)))",
        "MULTIPOLYGON (((0 0, 9 0, 10 10, 0 10, 0 0)))",
        3.0,
    );
}

// Test 6 (Z/M dimension preservation) is skipped: GeoRepair is 2D-only.

#[test]
fn rpr_test_7_all_exact_dups_single_entry() {
    assert_seq_eq("LINESTRING (3 7, 3 7, 3 7, 3 7)", "LINESTRING (3 7)", 0.0);
}

#[test]
fn rpr_test_8_all_within_tolerance_single_entry() {
    assert_seq_eq("LINESTRING (3 7, 3.1 7.1, 3.2 7.2, 3.3 7.3)", "LINESTRING (3 7)", 1.0);
}

#[test]
fn rpr_test_9_line_collapses_to_empty() {
    assert_geom_eq("LINESTRING (0 0, 0 1, 0 2, 0 3)", "LINESTRING EMPTY", 14.0);
}

#[test]
fn rpr_test_10_small_hole_collapses_away() {
    assert_geom_eq(
        "POLYGON ((0 0, 9 0, 10 0, 10 10, 0 10, 0 1, 0 0), (5 5, 5 6, 6 6, 6 5, 5 5))",
        "POLYGON ((0 0, 9 0, 10 10, 0 10, 0 0))",
        3.0,
    );
}

#[test]
fn rpr_test_11_small_exterior_collapses_to_degenerate() {
    // The GEOS expectation here is a 3-coordinate ring: the repair step pops
    // the within-tolerance (0 1) and re-attaches the original end (0 0).
    assert_geom_eq(
        "POLYGON ((0 0, 9 0, 10 0, 10 10, 0 10, 0 1, 0 0))",
        "POLYGON ((0 0, 10 10, 0 0))",
        12.0,
    );
}

#[test]
fn rpr_test_12_shell_collapses_to_empty_polygon() {
    assert_geom_eq(
        "POLYGON ((0 0, 9 0, 10 0, 10 10, 0 10, 0 1, 0 0))",
        "POLYGON EMPTY",
        22.0,
    );
}

#[test]
fn rpr_test_13_invalid_coords_not_replaced() {
    assert_geom_eq("LINESTRING (0 0, 0 Inf, 1 1, Inf 0)", "LINESTRING (0 0, 1 1)", 1.0);
}

#[test]
fn rpr_test_14_filters_to_one_point_is_empty() {
    assert_geom_eq("LINESTRING (0 0, 0 Inf, 1 1)", "LINESTRING EMPTY", 2.0);
}

#[test]
fn rpr_test_15_invalid_coords_at_start_end() {
    assert_geom_eq(
        "POLYGON ((Inf Inf, 0 0, 10 0, 10 10, 0 10, 0 0, Inf Inf))",
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))",
        2.0,
    );
}

#[test]
fn rpr_test_16_ring_with_invalid_coords_collapses() {
    assert_geom_eq(
        "POLYGON ((Inf Inf, 0 0, 10 0, 10 10, 0 10, 0 0, Inf Inf))",
        "POLYGON EMPTY",
        22.0,
    );
}

#[test]
fn rpr_test_17_tiny_hole_collapse_no_error() {
    // GH-1293: a valid polygon whose tiny hole collapses to a single point
    // must not error; the hole is dropped and the shell is preserved.
    // GEOS compares normalized geometries, so the ring rotation in the
    // expected WKT is not part of the assertion.
    assert_geom_eq(
        "POLYGON ((139770.26822331376024522 188334.00010800323798321, 139769.5 188338.01162790699163452, 139769.5 188338.3723930635896977, 139769.5 188338.5, 139769.81343283582828008 188338.5, 139770.375 188339.375, 139772.39924806414637715 188340.26989983080420643, 139770.26822331376024522 188334.00010800323798321),(139769.75256541155977175 188338.40516005983226933, 139769.75256541153066792 188338.40516005983226933, 139769.75256541153066792 188338.4051600598031655, 139769.75256541155977175 188338.40516005983226933))",
        "POLYGON ((139769.5 188338.011627907, 139769.5 188338.3723930636, 139769.5 188338.5, 139769.81343283583 188338.5, 139770.375 188339.375, 139772.39924806415 188340.2698998308, 139770.26822331376 188334.00010800324, 139769.5 188338.011627907))",
        1e-8,
    );
}

// =========================================================================
// ValidClosedRingTest (5 cases; ported from JTS)
// =========================================================================

/// The four GEOS validity tests are rings; standalone rings are parsed as
/// LineStrings by our reader, so ring semantics are exercised by wrapping
/// them as polygon exteriors.
fn ring_as_polygon(wkt: &str) -> Polygon<f64> {
    match geom_from_wkt(wkt) {
        Geometry::LineString(ls) => Polygon::new(ls, Vec::new()),
        other => panic!("expected linestring, got {other:?}"),
    }
}

fn assert_has_error(geom: &Geometry<f64>, want: GeometryValidationError) {
    let result = validate(geom);
    assert!(!result.valid, "expected invalid, got {result:?}");
    assert!(
        result.errors.contains(&want),
        "expected {want:?} in errors, got {:?}",
        result.errors
    );
}

#[test]
fn vcr_test_1_open_linear_ring_invalid() {
    // GEOS: LINEARRING (0 0, 0 10, 10 10, 10 0, 0 0) with the first point
    // perturbed by +0.0001 on x is invalid (ring not closed). GEOS rejects
    // the open ring at LinearRing construction (IllegalArgumentException).
    //
    // Our model: geo-types Polygon::new auto-closes rings, so an open ring
    // cannot be represented. The forced closing edge (0 0)->(0.0001 0)
    // overlaps the tail of the bottom edge (10 0)->(0 0) - (0.0001 0) lies
    // on that segment - so the ring is flagged SelfIntersection. Same
    // verdict as GEOS (invalid), different mechanism, documented divergence.
    let g = ring_as_polygon("LINESTRING (0 0, 0 10, 10 10, 10 0, 0 0)");
    let mut ring = g.exterior().clone();
    ring.0[0].x += 0.0001;
    let g = Geometry::Polygon(Polygon::new(ring, Vec::new()));
    assert!(!validate(&g).valid, "open ring must be invalid: {g:?}");
}

#[test]
fn vcr_test_2_closed_linear_ring_valid_in_geos_masked() {
    // GEOS: closed ring is valid (orientation is irrelevant to GEOS).
    // Ours: CW ring -> WrongOrientation (documented masked class); repair
    // restores validity with the area preserved.
    let g = Geometry::Polygon(ring_as_polygon("LINESTRING (0 0, 0 10, 10 10, 10 0, 0 0)"));
    assert_has_error(&g, GeometryValidationError::WrongOrientation);
    let fixed = g.make_valid();
    assert!(validate(&fixed).valid, "repair must restore validity: {fixed:?}");
    assert!(
        (geo::Area::unsigned_area(&fixed) - 100.0).abs() < 1e-9,
        "area must be preserved: {}",
        geo::Area::unsigned_area(&fixed)
    );
}

#[test]
fn vcr_test_3_open_polygon_shell_invalid() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))");
    let Geometry::Polygon(p) = g else { unreachable!() };
    let mut ring = p.exterior().clone();
    ring.0[0].x += 0.0001;
    let g = Geometry::Polygon(Polygon::new(ring, p.interiors().to_vec()));
    assert!(!validate(&g).valid, "open shell must be invalid");
}

#[test]
fn vcr_test_4_open_polygon_hole_invalid() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0), (1 1, 2 1, 2 2, 1 2, 1 1))");
    let Geometry::Polygon(p) = g else { unreachable!() };
    let mut hole = p.interiors()[0].clone();
    hole.0[0].x += 0.0001;
    let g = Geometry::Polygon(Polygon::new(
        p.exterior().clone(),
        vec![hole],
    ));
    assert!(!validate(&g).valid, "open hole must be invalid");
}

#[test]
fn vcr_test_5_closed_polygon_valid_in_geos_masked() {
    // GEOS: valid. Ours: CW shell -> WrongOrientation (documented masked
    // class); repair restores validity with the area preserved.
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))");
    assert_has_error(&g, GeometryValidationError::WrongOrientation);
    let fixed = g.make_valid();
    assert!(validate(&fixed).valid, "repair must restore validity: {fixed:?}");
    assert!(
        (geo::Area::unsigned_area(&fixed) - 100.0).abs() < 1e-9,
        "area must be preserved: {}",
        geo::Area::unsigned_area(&fixed)
    );
}

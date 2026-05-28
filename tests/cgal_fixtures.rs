//! CGAL-derived geometry repair regression tests.
//!
//! Test cases ported from CGAL test data files:
//! - `Boolean_set_operations_2/test/Boolean_set_operations_2/data/validation/`
//! - `Polygon/test/Polygon/data/`
//!
//! Each test verifies that MakeValid produces valid output from
//! known CGAL validation/invalidation test scenarios.

use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fn cfg_auto() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Auto,
        ..Default::default()
    }
}

fn cfg_arrange() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    }
}

fn cfg_structure() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    }
}

fn assert_valid(g: &Geometry<f64>) {
    use geo::validation::Validation;
    assert!(
        g.check_validation().is_ok(),
        "expected valid, got: {:?}",
        g.check_validation()
    );
}

fn assert_not_empty(g: &Geometry<f64>) {
    let is_empty = matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty())
        || matches!(g, Geometry::MultiPolygon(mp) if mp.0.is_empty())
        || matches!(g, Geometry::MultiLineString(mls) if mls.0.is_empty())
        || matches!(g, Geometry::MultiPoint(mp) if mp.0.is_empty());
    assert!(!is_empty, "expected non-empty, got: {:?}", g);
}

// =========================================================================
// CGAL validation test data port
// -------------------------------------------------------------------------
// Non-self-intersecting hole inside square (CGAL test1.dat, expected valid)
// =========================================================================

fn square_shell() -> LineString<f64> {
    LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 8.0, y: 0.0 },
        Coord { x: 8.0, y: 8.0 },
        Coord { x: 0.0, y: 8.0 },
    ])
}

#[test]
fn cgal_self_touching_hole_inside_shell() {
    // CGAL test1.dat: Hole self-touches at (3,1), valid in CGAL
    // Hole: (1,1)-(2,2)-(3,1)-(4,2)-(5,1)-(3,1)
    let poly = Polygon::new(
        square_shell(),
        vec![LineString::new(vec![
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 3.0, y: 1.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 5.0, y: 1.0 },
            Coord { x: 3.0, y: 1.0 },
        ])],
    );
    for cfg in [cfg_auto(), cfg_arrange(), cfg_structure()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid(&result);
    }
}

#[test]
fn cgal_hole_outside_shell_touching_at_one_vertex() {
    // CGAL test2.dat: Hole with one vertex outside shell at (4,12), invalid
    // Shell: (0,0)-(8,0)-(8,8)-(6,8)-(2,8)-(0,8)
    // Hole: (1,6)-(2,8)-(4,12)-(6,8)-(7,6)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 8.0, y: 0.0 },
            Coord { x: 8.0, y: 8.0 },
            Coord { x: 6.0, y: 8.0 },
            Coord { x: 2.0, y: 8.0 },
            Coord { x: 0.0, y: 8.0 },
        ]),
        vec![LineString::new(vec![
            Coord { x: 1.0, y: 6.0 },
            Coord { x: 2.0, y: 8.0 },
            Coord { x: 4.0, y: 12.0 },
            Coord { x: 6.0, y: 8.0 },
            Coord { x: 7.0, y: 6.0 },
        ])],
    );
    for cfg in [cfg_auto(), cfg_arrange(), cfg_structure()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid(&result);
    }
}

#[test]
fn cgal_hole_shares_edge_with_shell() {
    // CGAL test6.dat: Hole shares edge (2,0)-(4,0) with shell, invalid
    // Shell: (0,0)-(2,0)-(4,0)-(8,0)-(8,8)-(0,8)
    // Hole: (2,0)-(2,6)-(4,6)-(4,0)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 4.0, y: 0.0 },
            Coord { x: 8.0, y: 0.0 },
            Coord { x: 8.0, y: 8.0 },
            Coord { x: 0.0, y: 8.0 },
        ]),
        vec![LineString::new(vec![
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 2.0, y: 6.0 },
            Coord { x: 4.0, y: 6.0 },
            Coord { x: 4.0, y: 0.0 },
        ])],
    );
    for cfg in [cfg_auto(), cfg_arrange(), cfg_structure()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid(&result);
    }
}

#[test]
fn cgal_self_intersecting_no_holes() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 6.0, y: 4.0 },
            Coord { x: 6.0, y: 0.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 2.0, y: 4.0 },
        ]),
        Vec::new(),
    );
    // Auto and Arrange should produce non-empty output
    for cfg in [cfg_auto(), cfg_arrange()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_not_empty(&result);
        assert_valid(&result);
    }
    // Structure may collapse this to empty
    assert_valid(&poly.make_valid_with_config(&cfg_structure()));
}

#[test]
fn cgal_valid_polygon_with_touch_at_vertex() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 6.0, y: 0.0 },
            Coord { x: 6.0, y: 4.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 2.0, y: 4.0 },
        ]),
        Vec::new(),
    );
    // Auto/Arrange should handle this self-touching vertex.
    // Structure may detect it as self-intersecting and fall back.
    for cfg in [cfg_auto(), cfg_arrange()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid(&result);
    }
}

#[test]
fn cgal_polygon_holes_touching_valid() {
    // CGAL test9.dat: Valid polygon with 2 holes touching at point (4,4)
    // Shell: (0,0)-(4,0)-(8,0)-(8,8)-(0,8)
    // Hole1: (4,0)-(2,2)-(4,4)-(4,2)-(4,0)
    // Hole2: (4,4)-(2,6)-(6,6)-(4,4)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 4.0, y: 0.0 },
            Coord { x: 8.0, y: 0.0 },
            Coord { x: 8.0, y: 8.0 },
            Coord { x: 0.0, y: 8.0 },
        ]),
        vec![
            LineString::new(vec![
                Coord { x: 4.0, y: 0.0 },
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 4.0, y: 4.0 },
                Coord { x: 4.0, y: 2.0 },
            ]),
            LineString::new(vec![
                Coord { x: 4.0, y: 4.0 },
                Coord { x: 2.0, y: 6.0 },
                Coord { x: 6.0, y: 6.0 },
            ]),
        ],
    );
    for cfg in [cfg_auto(), cfg_arrange(), cfg_structure()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid(&result);
    }
}

#[test]
fn cgal_polygon_with_repeated_mid_ring_vertex() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 5.0 },
            Coord { x: 1.0, y: 3.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 0.0, y: 5.0 },
            Coord { x: 3.0, y: 1.0 },
            Coord { x: 4.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    // Auto and Arrange should handle this repeated-vertex polygon
    for cfg in [cfg_auto(), cfg_arrange()] {
        assert_not_empty(&poly.make_valid_with_config(&cfg));
        assert_valid(&poly.make_valid_with_config(&cfg));
    }
    assert_valid(&poly.make_valid_with_config(&cfg_structure()));
}

#[test]
fn cgal_self_intersecting_polygon_with_vertex_repeat() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 2.0, y: 1.0 },
            Coord { x: 3.0, y: 0.0 },
            Coord { x: 5.0, y: 4.0 },
            Coord { x: 2.0, y: 1.0 },
            Coord { x: 1.0, y: 3.0 },
            Coord { x: 0.0, y: 2.0 },
        ]),
        Vec::new(),
    );
    for cfg in [cfg_auto(), cfg_arrange()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_not_empty(&result);
        assert_valid(&result);
    }
    assert_valid(&poly.make_valid_with_config(&cfg_structure()));
}

#[test]
fn cgal_bowtie_simple3() {
    // simple3.dat: Self-intersecting bowtie with line along x-axis
    // (0,0)-(1,0)-(2,0)-(2,-1)-(2,1)-(0,1)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 2.0, y: -1.0 },
            Coord { x: 2.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
        ]),
        Vec::new(),
    );
    for cfg in [cfg_auto(), cfg_arrange()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_not_empty(&result);
        assert_valid(&result);
    }
    assert_valid(&poly.make_valid_with_config(&cfg_structure()));
}

#[test]
fn cgal_collinear_overlap_simple4() {
    // simple4.dat: Self-intersecting with collinear overlap
    // (0,0)-(1,0)-(1,2)-(1,1)-(0,1)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
        ]),
        Vec::new(),
    );
    for cfg in [cfg_auto(), cfg_arrange()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_not_empty(&result);
        assert_valid(&result);
    }
    assert_valid(&poly.make_valid_with_config(&cfg_structure()));
}

#[test]
fn cgal_backtracking_simple11() {
    // simple11.dat: Non-simple polygon with backtracking
    // (1,0)-(2,6)-(3,3)-(4,5)-(5,4)-(0,1)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 6.0 },
            Coord { x: 3.0, y: 3.0 },
            Coord { x: 4.0, y: 5.0 },
            Coord { x: 5.0, y: 4.0 },
            Coord { x: 0.0, y: 1.0 },
        ]),
        Vec::new(),
    );
    for cfg in [cfg_auto(), cfg_arrange()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_not_empty(&result);
        assert_valid(&result);
    }
    assert_valid(&poly.make_valid_with_config(&cfg_structure()));
}

// =========================================================================
// CGAL hole connection tests (pgn_holes*.dat)
// -------------------------------------------------------------------------

#[test]
fn cgal_hole_connection_complex() {
    // pgn_holes1.dat: Complex outer boundary with 4 holes, each 3-4 vertices
    // Shell: L-shaped ring (0,0)-(2,0)-(4,0)-(4,2)-(4,4)-(2,4)-(0,4)-(0,2)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 4.0, y: 4.0 },
            Coord { x: 2.0, y: 4.0 },
            Coord { x: 0.0, y: 4.0 },
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 4.0, y: 0.0 },
            Coord { x: 4.0, y: 2.0 },
        ]),
        vec![
            LineString::new(vec![
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 2.0, y: 1.0 },
                Coord { x: 2.0, y: 0.0 },
            ]),
            LineString::new(vec![
                Coord { x: 0.0, y: 2.0 },
                Coord { x: 1.0, y: 3.0 },
                Coord { x: 1.0, y: 2.0 },
            ]),
            LineString::new(vec![
                Coord { x: 1.0, y: 3.0 },
                Coord { x: 2.0, y: 4.0 },
                Coord { x: 2.0, y: 3.0 },
            ]),
            LineString::new(vec![
                Coord { x: 4.0, y: 2.0 },
                Coord { x: 3.0, y: 1.0 },
                Coord { x: 3.0, y: 2.0 },
            ]),
        ],
    );
    for cfg in [cfg_auto(), cfg_arrange(), cfg_structure()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid(&result);
    }
}

#[test]
fn cgal_hole_covers_interior() {
    // pgn_holes2.dat: Hole covers most of interior, valid
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 7.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 7.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 7.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 7.0 },
        ]),
        vec![LineString::new(vec![
            Coord { x: 10.0, y: 7.0 },
            Coord { x: 7.0, y: 0.0 },
            Coord { x: 0.0, y: 7.0 },
            Coord { x: 7.0, y: 10.0 },
        ])],
    );
    for cfg in [cfg_auto(), cfg_arrange(), cfg_structure()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid(&result);
    }
}

#[test]
fn cgal_valid_polygon_simple_convex() {
    // simple1.dat: Simple convex quadrilateral
    // (0,0)-(1,0)-(2,0)-(2,1)-(0,1)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 2.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
    // Should remain unchanged
    assert!(
        matches!(result, Geometry::Polygon(_)),
        "expected Polygon, got: {:?}",
        result
    );
}

#[test]
fn cgal_valid_polygon_star_simple12() {
    // simple12.dat: Complex valid polygon (19 vertices, concave)
    // (0,3)-(1,3)-(1,4)-(3,4)-(3,2)-(2,3.5)-(2,3)-(1.5,3.5)-(1,2)-(4,1)-
    // (7,3)-(6,5)-(4,2)-(4.5,2)-(4,1.5)-(3.5,3)-(3.5,4.5)-(4,5)-(0,5)
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 3.0 },
            Coord { x: 1.0, y: 3.0 },
            Coord { x: 1.0, y: 4.0 },
            Coord { x: 3.0, y: 4.0 },
            Coord { x: 3.0, y: 2.0 },
            Coord { x: 2.0, y: 3.5 },
            Coord { x: 2.0, y: 3.0 },
            Coord { x: 1.5, y: 3.5 },
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 4.0, y: 1.0 },
            Coord { x: 7.0, y: 3.0 },
            Coord { x: 6.0, y: 5.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 4.5, y: 2.0 },
            Coord { x: 4.0, y: 1.5 },
            Coord { x: 3.5, y: 3.0 },
            Coord { x: 3.5, y: 4.5 },
            Coord { x: 4.0, y: 5.0 },
            Coord { x: 0.0, y: 5.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn cgal_holes_connected_via_bridges_pgn3() {
    // pgn_holes3.dat: Multiple holes with complex connectivity
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 10.0, y: 6.0 },
            Coord { x: 5.0, y: 6.0 },
            Coord { x: 0.0, y: 6.0 },
            Coord { x: 0.0, y: 3.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 5.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 3.0 },
        ]),
        vec![
            LineString::new(vec![
                Coord { x: 4.0, y: 3.0 },
                Coord { x: 5.0, y: 4.0 },
                Coord { x: 5.0, y: 3.0 },
            ]),
            LineString::new(vec![
                Coord { x: 4.0, y: 2.0 },
                Coord { x: 5.0, y: 2.0 },
                Coord { x: 5.0, y: 1.0 },
            ]),
            LineString::new(vec![
                Coord { x: 0.0, y: 3.0 },
                Coord { x: 5.0, y: 6.0 },
                Coord { x: 2.0, y: 3.0 },
                Coord { x: 5.0, y: 0.0 },
            ]),
            LineString::new(vec![
                Coord { x: 8.0, y: 3.0 },
                Coord { x: 5.0, y: 6.0 },
                Coord { x: 10.0, y: 3.0 },
                Coord { x: 5.0, y: 0.0 },
            ]),
        ],
    );
    for cfg in [cfg_auto(), cfg_arrange(), cfg_structure()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid(&result);
    }
}

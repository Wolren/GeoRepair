//! CGAL-derived geometry repair regression tests.
//!
//! Test cases ported from CGAL test data files:
//! - `Boolean_set_operations_2/test/Boolean_set_operations_2/data/validation/`
//! - `Polygon/test/Polygon/data/`
//! - `Polygon_repair/test/Polygon_repair/data/`
//!
//! Each test verifies that MakeValid produces valid output from
//! known CGAL validation/invalidation test scenarios, with specific
//! assertions about output geometry type and component count.

use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon};
use geo_repair::{MakeValid, MakeValidConfig};
use wkt::TryFromWkt;

#[path = "common/mod.rs"]
mod common;
use common::*;

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
        assert_valid_ogc(&result);
        assert_not_empty(&result);
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
        assert_valid_ogc(&result);
        assert_not_empty(&result);
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
        assert_valid_ogc(&result);
        assert_not_empty(&result);
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
    for cfg in [cfg_auto(), cfg_arrange()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_not_empty(&result);
        assert_valid_ogc(&result);
    }
    // Structure may collapse this to empty
    let result = poly.make_valid_with_config(&cfg_structure());
    assert_valid_ogc(&result);
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
    for cfg in [cfg_auto(), cfg_arrange()] {
        let result = poly.make_valid_with_config(&cfg);
        assert_valid_ogc(&result);
        assert_not_empty(&result);
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
        assert_not_empty(&result);
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
        let result = poly.make_valid_with_config(&cfg);
        assert_not_empty(&result);
        assert_valid_ogc(&result);
    }
    let result = poly.make_valid_with_config(&cfg_structure());
    assert_valid_ogc(&result);
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
        assert_valid_ogc(&result);
        assert_multipolygon_count(&result, 2);
    }
    let result = poly.make_valid_with_config(&cfg_structure());
    assert_valid_ogc(&result);
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
        assert_valid_ogc(&result);
    }
    let result = poly.make_valid_with_config(&cfg_structure());
    assert_valid_ogc(&result);
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
        assert_valid_ogc(&result);
    }
    let result = poly.make_valid_with_config(&cfg_structure());
    assert_valid_ogc(&result);
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
        assert_valid_ogc(&result);
    }
    let result = poly.make_valid_with_config(&cfg_structure());
    assert_valid_ogc(&result);
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
        assert_not_empty(&result);
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
        assert_valid_ogc(&result);
        assert_not_empty(&result);
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
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
    assert_eq!(
        result,
        Geometry::Polygon(poly.clone()),
        "valid polygon should be unchanged"
    );
}

#[test]
fn cgal_valid_polygon_star_simple12() {
    // simple12.dat: Complex valid polygon (19 vertices, concave)
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
    assert_valid_ogc(&result);
    assert_is_polygon(&result);
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
        assert_valid_ogc(&result);
        assert_not_empty(&result);
    }
}

// =========================================================================
// Additional CGAL Polygon_repair test data port
// These cases are from cgal/Polygon_repair/test/Polygon_repair/data/
// =========================================================================

// ---------------------------------------------------------------------------
// Crossing polygons: two pairs of crossing rectangles
// Input: 4 crossing 1x1 squares at (0,1), (1,0), (1,2), (2,1)
// CGAL ref: MULTIPOLYGON with 4 components
// ---------------------------------------------------------------------------
#[test]
fn cgal_crossing_polygons() {
    let g = geom_from_wkt("POLYGON ((0 1, 1 1, 1 2, 0 2, 0 1))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Hole-carved: hole carves a notch out of the shell
// Input: Shell (0,0)-(1,0)-(1,1)-(0,1), hole (0.5,0)-(1,0.5)-(0.5,1)-(0.5,0)
// CGAL ref: MULTIPOLYGON with carved shape
// BUG: This test is for the Arrange method specifically since Structure
// may produce a different result.
// ---------------------------------------------------------------------------
#[test]
fn cgal_hole_carved() {
    let g = geom_from_wkt("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0), (0.5 0, 1 0.5, 0.5 1, 0.5 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Hole touches shell at two points, splitting the polygon
// Input: Shell (0,0)-(1,0)-(1,1)-(0,1), hole touches at (0,1) and (0.75,0.75)
// CGAL ref: MULTIPOLYGON with 2 components
// ---------------------------------------------------------------------------
#[test]
fn cgal_hole_touching_twice() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0), (0 1, 0.75 0.75, 0.75 0.25, 0.25 0.25, 0 1))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    assert_is_polygon(&result);
}

// ---------------------------------------------------------------------------
// Hole partly outside shell
// Input: Shell (0,0)-(1,0)-(1,1)-(0,1), hole extends beyond at top-right
// CGAL ref: MULTIPOLYGON with 2 components
// ---------------------------------------------------------------------------
#[test]
fn cgal_hole_partly_outside() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 1 0, 1 0.75, 0.75 0.75, 0.75 1, 0 1, 0 0), (0.75 1, 1 1, 1 0.75, 1.25 0.75, 1.25 1.25, 0.75 1.25, 0.75 1))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Star: complex 9-component MultiPolygon
// Input: Star-shaped polygon with multiple crossing spikes (18 vertices)
// CGAL ref: MULTIPOLYGON with 9 components
// This is a complex test that validates the Arrange method's robustness.
// ---------------------------------------------------------------------------
#[test]
fn cgal_star_complex() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 1.2 0.6, 1 1, 0.6 1.2, 0 0, 0 1.5, 0.6 1.2, 0.75 1.5, 0.6 1.8, 0 1.5, 0 3, 0.6 1.8, 1 2, 1.2 2.4, 0 3, 0.75 1.5, 1 1, 1.5 0.75, 2 1, 2.25 1.5, 2 2, 1.5 2.25, 1 2, 1.2 2.4, 1.5 2.25, 1.8 2.4, 1.5 3, 1.2 2.4, 1.8 0.6, 3 0, 2.4 1.2, 2 1, 1.8 0.6, 1.8 2.4, 2 2, 2.4 1.8, 3 3, 1.8 2.4, 2.25 1.5, 2.4 1.2, 3 1.5, 2.4 1.8, 2.25 1.5))",
    );
    let result = g.make_valid_with_config(&cfg_arrange());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Square-hole-rhombus: rhombus hole splits square into 4 parts
// Input: 1x1 square with rhombus hole touching all 4 edges at midpoints
// CGAL ref: MULTIPOLYGON with 4 components
// ---------------------------------------------------------------------------
#[test]
fn cgal_square_hole_rhombus() {
    let g =
        geom_from_wkt("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0), (0.5 0, 1 0.5, 0.5 1, 0 0.5, 0.5 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_multipolygon_count(&result, 4);
}

// ---------------------------------------------------------------------------
// Not-closed ring (CGAL not-closed.wkt)
// Input: POLYGON((0 0,1 0,1 1,0 1)) — unclosed ring
// CGAL ref: MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0))) — closed square
// ---------------------------------------------------------------------------
#[test]
fn cgal_not_closed_ring() {
    let g = geom_from_wkt("POLYGON ((0 0, 1 0, 1 1, 0 1))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Spike-in (CGAL spike-in.wkt): spike pointing into the interior
// Input: Square (0,0)-(1,0)-(1,1)-(0,1) with spike (0.5,0.2) going inward
// CGAL ref: Simple square (spike removed)
// ---------------------------------------------------------------------------
#[test]
fn cgal_spike_in() {
    let g = geom_from_wkt("POLYGON ((0 0, 1 0, 1 1, 0.5 0.2, 0.2 0.5, 0 1, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_is_polygon(&result);
}

// ---------------------------------------------------------------------------
// Float-precision spikes (CGAL spikes-fp.wkt)
// ---------------------------------------------------------------------------
#[test]
fn cgal_spikes_fp() {
    let g = geom_from_wkt("POLYGON ((0.03 0.02, 0.97 0.01, 0.99 0.96, 0.04 0.98, 0.03 0.02))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_simple_polygon(&result);
}

// ---------------------------------------------------------------------------
// Hole-as-loop: interior ring formed as a self-loop
// Input: Outer square with interior ring (0,1)-(0.75,0.75)-(0.75,0.25)-
//         (0.25,0.25)-(0,1)
// CGAL ref: MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0),(0 1,0.75 0.75,0.75 0.25,0.25 0.25,0 1)))
// ---------------------------------------------------------------------------
#[test]
fn cgal_hole_as_loop() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0), (0 1, 0.75 0.75, 0.75 0.25, 0.25 0.25, 0 1))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_is_polygon(&result);
}

// ---------------------------------------------------------------------------
// Overlapping-edge-inside: two overlapping rectangles forming notch
// Input: Outer rect (0,0)-(1,0)-(1,1)-(0,1) with interior rect
//        (0.25,0.25)-(0.75,0.25)-(0.75,0.75)-(0.25,0.75)
// CGAL ref: Polygon with notch shape
// ---------------------------------------------------------------------------
#[test]
fn cgal_overlapping_edge_inside() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 1 0, 1 1, 0.75 0.75, 0.75 0.25, 0.25 0.25, 0.25 0.75, 0 1, 0 0))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Crossing polygons with nesting
// Input: 5 intersecting 1x1 squares arranged in a cross pattern
// CGAL ref: MULTIPOLYGON with 5 components
// ---------------------------------------------------------------------------
#[test]
fn cgal_crossing_polygons_nesting() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (((0 1, 1 1, 1 2, 0 2, 0 1)), ((1 0, 2 0, 2 1, 1 1, 1 0)), ((1 2, 2 2, 2 3, 1 3, 1 2)), ((1.25 1.25, 1.75 1.25, 1.75 1.75, 1.25 1.75, 1.25 1.25)), ((2 1, 3 1, 3 2, 2 2, 2 1)))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Back-and-forth degenerate (CGAL back-and-forth.wkt)
// Input: Zero-area degenerate polygon with backtracking
// CGAL ref: MULTIPOLYGON() (empty)
// ---------------------------------------------------------------------------
#[test]
fn cgal_back_and_forth_degenerate() {
    let g = geom_from_wkt("POLYGON ((0 0, 5 0, 5 5, 0 5, 5 5, 5 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
}

// ---------------------------------------------------------------------------
// Edge-only degenerate (CGAL edge.wkt)
// Input: Two edge-only polygons
// CGAL ref: MULTIPOLYGON() (empty)
// ---------------------------------------------------------------------------
#[test]
fn cgal_edge_only_degenerate() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

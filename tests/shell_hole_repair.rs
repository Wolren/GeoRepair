//! Polygon shell/hole repair scenarios ported from GEOS GeometryFixer
//! and CGAL validation tests.
//!
//! Covers self-touching rings, hole edge cases, shell collapse,
//! complex invalid polygons, PostGIS regressions, and bowtie variants.

#[allow(unused_imports)]
use geo::{Coord, Geometry, GeometryCollection, LineString, MultiPolygon, Polygon};
use geo_repair::{MakeValid, MakeValidConfig};
use wkt::TryFromWkt;

#[path = "common/mod.rs"]
mod common;
use common::*;

// =========================================================================
// SECTION 1: Self-touching rings forming holes
// (GEOS ValidSelfTouchingRingFormingHoleTest)
// =========================================================================

#[test]
fn test_self_touching_ring_forming_hole() {
    let g = geom_from_wkt(
        "POLYGON ((100 0, 100 100, 200 100, 200 0, 150 0, 170 40, 130 40, 150 0, 100 0))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_self_touch_shell_hole() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 0 340, 320 340, 320 0, 120 0, 180 100, 60 100, 120 0, 0 0), \
                  (80 300, 80 180, 200 180, 200 240, 280 200, 280 280, 200 240, 200 300, 80 300))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_self_touch_disconnected_interior() {
    let g = geom_from_wkt("POLYGON ((40 180, 40 60, 240 60, 240 180, 140 60, 40 180))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_self_touch_at_vertex() {
    let g = geom_from_wkt(
        "POLYGON ((20 20, 20 100, 140 100, 140 180, 260 180, 260 100, 140 100, 140 20, 20 20))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_shell_crossing() {
    let g = geom_from_wkt("POLYGON ((20 20, 120 20, 120 220, 240 220, 240 120, 20 120, 20 20))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_holes_meeting_at_point() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 10.0, y: 90.0 },
            Coord { x: 90.0, y: 90.0 },
            Coord { x: 90.0, y: 10.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 90.0 },
        ]),
        vec![
            LineString::new(vec![
                Coord { x: 40.0, y: 80.0 },
                Coord { x: 60.0, y: 80.0 },
                Coord { x: 50.0, y: 50.0 },
                Coord { x: 40.0, y: 80.0 },
            ]),
            LineString::new(vec![
                Coord { x: 20.0, y: 60.0 },
                Coord { x: 20.0, y: 40.0 },
                Coord { x: 50.0, y: 50.0 },
                Coord { x: 20.0, y: 60.0 },
            ]),
            LineString::new(vec![
                Coord { x: 40.0, y: 20.0 },
                Coord { x: 60.0, y: 20.0 },
                Coord { x: 50.0, y: 50.0 },
                Coord { x: 40.0, y: 20.0 },
            ]),
        ],
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 2: Hole edge cases
// (GEOS GeometryFixer + CGAL validation)
// =========================================================================

#[test]
fn test_hole_touching_two_places() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (0 2, 5 2, 5 8, 0 8, 0 2))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_hole_outside_shell() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (20 20, 25 20, 25 25, 20 25, 20 20))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_hole_overlaps_shell() {
    let g =
        geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (5 -5, 15 5, 5 15, -5 5, 5 -5))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
}

#[test]
fn test_holes_overlapping_each_other() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 20 0, 20 20, 0 20, 0 0), \
                  (3 3, 10 3, 10 10, 3 10, 3 3), \
                  (7 7, 17 7, 17 17, 7 17, 7 7))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_nested_holes() {
    let g = geom_from_wkt(
        "POLYGON ((0 0, 20 0, 20 20, 0 20, 0 0), \
                  (2 2, 18 2, 18 18, 2 18, 2 2), \
                  (6 6, 14 6, 14 14, 6 14, 6 6))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_multipolygon_count(&result, 1);
}

#[test]
fn test_hole_shares_edge_with_shell() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (0 0, 5 0, 5 5, 0 5, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_zero_area_hole_removed() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 4 2, 6 2, 2 2))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_polygon_pos_neg_overlap() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 5 5, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_hole_degenerate_collapse() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 2 8, 2 2))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 3: Shell collapse edge cases
// =========================================================================

#[test]
fn test_shell_collapse_to_line() {
    let g = geom_from_wkt("POLYGON ((0 0, 5 0, 10 0, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_shell_collapse_keep_collapsed() {
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    let g = geom_from_wkt("POLYGON ((0 0, 5 0, 10 0, 0 0))");
    let result = g.make_valid_with_config(&config);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_shell_collapse_nan() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: f64::NAN,
                y: 0.0,
            },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

#[test]
fn test_shell_collapse_nan_keep_collapsed() {
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: f64::NAN,
                y: 0.0,
            },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&config);
    assert_valid_ogc(&result);
}

// ---------------------------------------------------------------------------
// JTS-style shell collapse with backtrack (zero-area shell + valid hole)
// ---------------------------------------------------------------------------

#[test]
fn test_shell_backtrack_collapse() {
    let g = geom_from_wkt(
        "POLYGON ((10 10, 10 90, 90 90, 10 90, 10 10), \
                  (20 80, 60 80, 60 40, 20 40, 20 80))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_shell_backtrack_collapse_keep_collapsed() {
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    let g = geom_from_wkt(
        "POLYGON ((10 10, 10 90, 90 90, 10 90, 10 10), \
                  (20 80, 60 80, 60 40, 20 40, 20 80))",
    );
    let result = g.make_valid_with_config(&config);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// JTS-style hole collapse (valid shell + zero-area hole)
// ---------------------------------------------------------------------------

#[test]
fn test_hole_backtrack_collapse() {
    let g = geom_from_wkt(
        "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), \
                  (80 80, 20 80, 20 20, 20 80, 80 80))",
    );
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_hole_backtrack_collapse_keep_collapsed() {
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    let g = geom_from_wkt(
        "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), \
                  (80 80, 20 80, 20 20, 20 80, 80 80))",
    );
    let result = g.make_valid_with_config(&config);
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 4: Complex invalid polygons from CGAL validation
// =========================================================================

#[test]
fn test_cgal_self_intersect_dup_points() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 10 10, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_cgal_valid_holes() {
    let g = geom_from_wkt("POLYGON ((0 0, 20 0, 20 20, 0 20, 0 0), (5 5, 15 5, 15 15, 5 15, 5 5))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
    assert_eq!(
        result, g,
        "valid polygon with holes should pass through unchanged"
    );

    let result = g.make_valid_with_config(&cfg_structure());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_cgal_overlapping_boundary() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (0 0, 10 0, 10 10, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_is_empty(&result);
}

// =========================================================================
// SECTION 5: PostGIS / real-world regression cases
// =========================================================================

#[test]
fn test_postgis_complex_self_intersect() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 5 5, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_geos_issue_265() {
    let g = geom_from_wkt("POLYGON ((0 0, 1 0, 1 1, 0.5 0.5, 0 1, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

// =========================================================================
// SECTION 6: Bowtie variants
// =========================================================================

#[test]
fn test_bowtie_basic() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);

    let result = g.make_valid_with_config(&cfg_arrange());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_bowtie_offset() {
    let g = geom_from_wkt("POLYGON ((0 0, 10 10, 8 0, 2 10, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);

    let result = g.make_valid_with_config(&cfg_arrange());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

#[test]
fn test_bowtie_large_coords() {
    let g = geom_from_wkt("POLYGON ((0 0, 1000000 1000000, 1000000 0, 0 1000000, 0 0))");
    let result = g.make_valid_with_config(&cfg_auto());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);

    let result = g.make_valid_with_config(&cfg_arrange());
    assert_valid_ogc(&result);
    assert_valid_ogc(&result);
    assert_not_empty(&result);
}

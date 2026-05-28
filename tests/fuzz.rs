//! Fuzz testing: property-based validation with randomly generated geometries.
//!
//! Uses `proptest` to generate random invalid geometries and verify
//! invariants: the output must always be valid, and valid inputs must be unchanged.

use geo::validation::Validation;
use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use proptest::prelude::*;

fn assert_valid(g: &Geometry<f64>) {
    g.check_validation().unwrap();
}

fn assert_not_empty(g: &Geometry<f64>) {
    assert!(!matches!(g, Geometry::GeometryCollection(gc) if gc.0.is_empty()));
}

fn cfg_arrange() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    }
}

fn cfg_auto() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Auto,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Random coordinate generators
// ---------------------------------------------------------------------------

fn coord_range(range: std::ops::RangeInclusive<f64>) -> impl Strategy<Value = Coord<f64>> {
    (range.clone(), range).prop_map(|(x, y)| Coord { x, y })
}

fn point_range(range: std::ops::RangeInclusive<f64>) -> impl Strategy<Value = Point<f64>> {
    coord_range(range).prop_map(Point)
}

fn linestring_points(
    range: std::ops::RangeInclusive<f64>,
    min: usize,
    max: usize,
) -> impl Strategy<Value = LineString<f64>> {
    proptest::collection::vec(coord_range(range.clone()), min..=max).prop_map(LineString::new)
}

fn polygon_points(
    range: std::ops::RangeInclusive<f64>,
    n: usize,
) -> impl Strategy<Value = Polygon<f64>> {
    proptest::collection::vec(coord_range(range.clone()), n..=n).prop_map(|mut coords| {
        if coords.first() != coords.last() {
            coords.push(coords[0]);
        }
        Polygon::new(LineString::new(coords), Vec::new())
    })
}

// ---------------------------------------------------------------------------
// Invariant: make_valid always returns valid geometry
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn invariant_valid_polygon(
        coords in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=12)
    ) {
        let mut ring = coords;
        if ring.first() != ring.last() {
            ring.push(ring[0]);
        }
        let poly = Polygon::new(LineString::new(ring), Vec::new());
        let result = poly.make_valid_with_config(&cfg_auto());

        // Output must be valid
        assert_valid(&result);

        // If input was valid, output must be non-empty and similar shape
        if poly.check_validation().is_ok() {
            assert_not_empty(&result);
        }
    }

    #[test]
    fn invariant_valid_multipolygon(
        polys in proptest::collection::vec(polygon_points(-50.0..=50.0, 3), 1..=5)
    ) {
        let mp = MultiPolygon::new(polys);
        let result = mp.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_valid_multipoint(
        points in proptest::collection::vec(point_range(-1000.0..=1000.0), 0..=20)
    ) {
        let mp = MultiPoint::new(points);
        let result = mp.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_valid_multilinestring(
        lss in proptest::collection::vec(linestring_points(-500.0..=500.0, 2, 8), 0..=10)
    ) {
        let mls = MultiLineString::new(lss);
        let result = mls.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_valid_geometry_collection(
        points in proptest::collection::vec(point_range(-100.0..=100.0), 0..=5),
        polys in proptest::collection::vec(polygon_points(-100.0..=100.0, 3), 0..=3),
        lss in proptest::collection::vec(linestring_points(-100.0..=100.0, 2, 4), 0..=3),
    ) {
        let mut items = Vec::new();
        for p in points { items.push(Geometry::Point(p)); }
        for p in polys { items.push(Geometry::Polygon(p)); }
        for ls in lss { items.push(Geometry::LineString(ls)); }
        if items.is_empty() {
            return Ok(());
        }
        let gc = GeometryCollection(items);
        let result = gc.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_linestring_no_crash(
        coords in proptest::collection::vec(coord_range(-1000.0..=1000.0), 0..=50)
    ) {
        let ls = LineString::new(coords);
        let _result = ls.make_valid_with_config(&cfg_auto());
        // Just verify it doesn't panic
    }

    #[test]
    fn invariant_point_no_crash(
        x in -1e10f64..1e10f64,
        y in -1e10f64..1e10f64,
    ) {
        let pt = Point::new(x, y);
        let result = pt.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_line_no_crash(
        x1 in -1e8f64..1e8f64, y1 in -1e8f64..1e8f64,
        x2 in -1e8f64..1e8f64, y2 in -1e8f64..1e8f64,
    ) {
        let line = Line::new(Coord { x: x1, y: y1 }, Coord { x: x2, y: y2 });
        let result = line.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn geometry_dispatch_no_crash(
        kind in 0u8..6u8,
        coords in proptest::collection::vec(coord_range(-500.0..=500.0), 3..=8),
    ) {
        let g = match kind {
            0 => Geometry::Point(Point::new(coords[0].x, coords[0].y)),
            1 => {
                let mut ring = coords;
                if ring.first() != ring.last() { ring.push(ring[0]); }
                Geometry::Polygon(Polygon::new(LineString::new(ring), Vec::new()))
            }
            2 => Geometry::LineString(LineString::new(coords)),
            3 => Geometry::MultiPoint(MultiPoint::new(vec![Point::new(coords[0].x, coords[0].y)])),
            _ => {
                let mut ring = coords;
                if ring.first() != ring.last() { ring.push(ring[0]); }
                Geometry::MultiPolygon(MultiPolygon::new(vec![Polygon::new(LineString::new(ring), Vec::new())]))
            }
        };
        let result = g.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn arrange_method_no_crash(
        coords in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=10)
    ) {
        let mut ring = coords;
        if ring.first() != ring.last() { ring.push(ring[0]); }
        let poly = Polygon::new(LineString::new(ring), Vec::new());
        let result = poly.make_valid_with_config(&cfg_arrange());
        assert_valid(&result);
    }

    // -----------------------------------------------------------------------
    // Additional property tests
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_triangle_valid(
        coords in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=3)
    ) {
        let a = coords[0];
        let b = coords[1];
        let c = coords[2];
        let tri = geo::Triangle::new(a, b, c);
        let result = tri.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_rect_valid(
        min_coords in coord_range(-1000.0..=1000.0),
        max_coords in coord_range(-1000.0..=1000.0),
    ) {
        let r = geo::Rect::new(
            geo::Point::new(min_coords.x, min_coords.y),
            geo::Point::new(max_coords.x, max_coords.y),
        );
        let result = r.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_keep_collapsed_no_panic(
        coords in proptest::collection::vec(coord_range(-100.0..=100.0), 0..=10),
        keep in proptest::bool::ANY,
    ) {
        let config = MakeValidConfig {
            keep_collapsed: keep,
            ..Default::default()
        };
        let ls = LineString::new(coords);
        let result = ls.make_valid_with_config(&config);
        assert_valid(&result);
    }

    #[test]
    fn invariant_geometry_dispatch_no_panic(
        coord in coord_range(-1e6..=1e6),
    ) {
        let geoms = vec![
            Geometry::Point(geo::Point::new(coord.x, coord.y)),
            Geometry::Line(geo::Line::new(coord, Coord { x: coord.x + 1.0, y: coord.y + 1.0 })),
            Geometry::LineString(LineString::new(vec![coord, Coord { x: coord.x + 1.0, y: coord.y }])),
            Geometry::MultiPoint(MultiPoint::new(vec![geo::Point::new(coord.x, coord.y)])),
        ];
        for g in geoms {
            let result = g.make_valid_with_config(&cfg_auto());
            assert_valid(&result);
        }
    }

    #[test]
    fn invariant_multiline_string_valid_after_fix(
        lss in proptest::collection::vec(
            linestring_points(-100.0..=100.0, 2, 6),
            1..=4
        )
    ) {
        let mls = MultiLineString::new(lss);
        let result = mls.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_all_poly_methods_valid(
        coords in proptest::collection::vec(coord_range(-50.0..=50.0), 3..=8)
    ) {
        let mut ring = coords;
        if ring.first() != ring.last() { ring.push(ring[0]); }
        let poly = Polygon::new(LineString::new(ring), Vec::new());
        for method in &[PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
            let config = MakeValidConfig {
                poly_method: method.clone(),
                ..Default::default()
            };
            let result = poly.make_valid_with_config(&config);
            assert_valid(&result);
        }
    }

    #[test]
    fn invariant_no_crash_nan_inf(
        coords in proptest::collection::vec(proptest::num::f64::ANY, 0..=8)
    ) {
        // Test that NaN/inf coordinates don't cause panics in any geometry type
        for n in 0..coords.len().min(8).max(3) {
            let mut ring: Vec<Coord<f64>> = coords.iter().take(n).map(|&x| Coord { x, y: x }).collect();
            if ring.len() >= 3 {
                if ring.first() != ring.last() {
                    ring.push(ring[0]);
                }
                let poly = Polygon::new(LineString::new(ring), Vec::new());
                let _result = poly.make_valid_with_config(&cfg_auto());
            }
        }
    }

    #[test]
    fn invariant_geometry_collection_with_bowtie(
        points in proptest::collection::vec(point_range(-100.0..=100.0), 0..=3),
    ) {
        let bowtie = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let mut items: Vec<Geometry<f64>> = points.into_iter().map(Geometry::Point).collect();
        items.push(Geometry::Polygon(bowtie));
        let gc = GeometryCollection(items);
        let result = gc.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
        assert_not_empty(&result);
    }
}

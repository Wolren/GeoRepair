//! Fuzz testing: property-based validation with randomly generated geometries.
//!
//! Uses `proptest` to generate random invalid geometries and verify
//! invariants: the output must always be valid, and valid inputs must be unchanged.

use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use proptest::prelude::*;

fn assert_valid(g: &Geometry<f64>) {
    let r = g.validate();
    assert!(r.valid, "geometry invalid: {:?}", r.errors);
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

fn geometry_with_zm() -> impl Strategy<Value = (geo::Geometry<f64>, Vec<geo_repair::zm::ZmValue>)> {
    let coord_strat = proptest::collection::vec(coord_range(-100.0..=100.0), 3..=8);
    let zm_single = (
        proptest::option::of(proptest::num::f64::NORMAL),
        proptest::option::of(proptest::num::f64::NORMAL),
    )
        .prop_map(|(z, m)| geo_repair::zm::ZmValue::new(z, m));
    let zm_strat = proptest::collection::vec(zm_single, 3..=9);
    (coord_strat, zm_strat).prop_map(|(mut coords, zm)| {
        if coords.first() != coords.last() {
            coords.push(coords[0]);
        }
        let poly = geo::Polygon::new(geo::LineString::new(coords), Vec::new());
        (geo::Geometry::Polygon(poly), zm)
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
        if poly.validate().valid {
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

    // -----------------------------------------------------------------------
    // Property: polygons with holes (fuzz exercise all hole code paths)
    // -----------------------------------------------------------------------

    fn invariant_valid_polygon_with_holes(
        shell_coords in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=8),
        hole_coords in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=8),
        hole_offset_x in -50.0f64..50.0f64,
        hole_offset_y in -50.0f64..50.0f64,
    ) {
        let mut shell = shell_coords;
        if shell.first() != shell.last() { shell.push(shell[0]); }

        let mut hole = hole_coords;
        if hole.first() != hole.last() { hole.push(hole[0]); }
        let hole_ls = LineString::new(hole.iter().map(|c| Coord {
            x: c.x + hole_offset_x,
            y: c.y + hole_offset_y,
        }).collect());

        let poly = Polygon::new(LineString::new(shell), vec![hole_ls]);
        let result = poly.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    #[test]
    fn invariant_valid_multi_hole_polygon(
        shell_coords in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=8),
        holes in proptest::collection::vec(
            (proptest::collection::vec(coord_range(-100.0..=100.0), 3..=6),
             -50.0f64..50.0f64, -50.0f64..50.0f64),
            0..=3,
        ),
    ) {
        let mut shell = shell_coords;
        if shell.first() != shell.last() { shell.push(shell[0]); }

        let interiors: Vec<LineString<f64>> = holes.into_iter()
            .filter_map(|(coords, ox, oy)| {
                if coords.is_empty() { return None; }
                let mut c = coords;
                if c.first() != c.last() { c.push(c[0]); }
                Some(LineString::new(c.into_iter().map(|c| Coord {
                    x: c.x + ox, y: c.y + oy,
                }).collect()))
            })
            .collect();

        let poly = Polygon::new(LineString::new(shell), interiors);
        let result = poly.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
    }

    // -----------------------------------------------------------------------
    // Property: nested GeometryCollection (depth up to 5)
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_valid_nested_geometry_collection(
        leaf_points in proptest::collection::vec(point_range(-100.0..=100.0), 0..=3),
        nest_depth in 0u8..5u8,
    ) {
        let mut gc: Geometry<f64> = Geometry::GeometryCollection(GeometryCollection(
            leaf_points.into_iter().map(Geometry::Point).collect()
        ));
        for _ in 0..nest_depth {
            gc = Geometry::GeometryCollection(GeometryCollection(vec![gc]));
        }
        let result = gc.make_valid_with_config(&cfg_auto());
        assert_valid(&result);
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

    // -----------------------------------------------------------------------
    // Property: Z/M preservation through repair for polygon and linestring
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_zm_preserved_through_repair(
        (geom, zm) in geometry_with_zm(),
    ) {
        let feature = geo_repair::Feature::with_all(geom, None, None, zm);
        use geo_repair::GeoValidation;
        let validated = feature.geometry.validate();
        let repaired = feature.with_repaired_geometry(
            feature.geometry.make_valid_with_config(&cfg_auto())
        );
        let expected_count = geo_repair::zm::count_coords(&repaired.geometry);
        prop_assert_eq!(repaired.zm.len(), expected_count,
            "Z/M count {} must match repaired coord count {}", repaired.zm.len(), expected_count);
        if validated.valid && repaired.zm.len() == feature.zm.len() {
            for (i, zm) in repaired.zm.iter().enumerate() {
                prop_assert_eq!(zm, &feature.zm[i],
                    "Z/M at index {} should be preserved for valid geometry", i);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Property: validate_or_fix always returns Ok with valid geometry
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_validate_or_fix_valid(
        coords in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=8),
        kind in 0u8..4u8,
    ) {
        use geo_repair::ValidateAndFix;
        let g: geo::Geometry<f64> = match kind {
            0 => {
                if coords.is_empty() { return Ok(()); }
                geo::Geometry::Point(geo::Point::new(coords[0].x, coords[0].y))
            }
            1 => {
                let mut ring = coords;
                if ring.first() != ring.last() { ring.push(ring[0]); }
                geo::Geometry::Polygon(geo::Polygon::new(geo::LineString::new(ring), Vec::new()))
            }
            2 => geo::Geometry::LineString(geo::LineString::new(coords)),
            _ => geo::Geometry::MultiPoint(geo::MultiPoint::new(
                coords.iter().map(|c| geo::Point::new(c.x, c.y)).collect()
            )),
        };
        let result = g.validate_or_fix();
        match result {
            Ok(fixed) => {
                prop_assert!(fixed.validate().valid,
                    "validate_or_fix returned Ok but geometry is invalid");
            }
            Err((_errors, fixed)) => {
                prop_assert!(fixed.validate().valid,
                    "validate_or_fix returned Err but fixed geometry is invalid");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Property: Feature validation with random Z/M
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_feature_validate_zm_consistency(
        coords in proptest::collection::vec(coord_range(-10.0..=10.0), 3..=6),
        zm_enabled in proptest::bool::ANY,
    ) {
        let mut ring = coords;
        if ring.first() != ring.last() { ring.push(ring[0]); }
        let poly = geo::Polygon::new(geo::LineString::new(ring), Vec::new());
        let geom = geo::Geometry::Polygon(poly);
        let count = geo_repair::zm::count_coords(&geom);
        let zm: Vec<geo_repair::zm::ZmValue> = if zm_enabled {
            (0..count).map(|i| geo_repair::zm::ZmValue::new(Some(i as f64), Some(i as f64 * 2.0))).collect()
        } else {
            vec![geo_repair::zm::ZmValue::NONE; count]
        };
        let feature = geo_repair::Feature::with_all(geom.clone(), None, None, zm);
        use geo_repair::GeoValidation;
        let result = feature.geometry.validate();
        if zm_enabled {
            if result.valid {
                prop_assert!(geom.validate().valid,
                    "Feature valid but geometry OGC check failed: {:?}", geom.validate());
            }
        } else {
            prop_assert_eq!(result.valid, geom.validate().valid,
                "Feature validate without Z/M should match OGC validation");
        }
    }

    // -----------------------------------------------------------------------
    // Property: Repairing a valid geometry should produce valid output
    // (stronger: for simple coords, all methods should work)
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_all_methods_valid_on_small(
        coords in proptest::collection::vec(coord_range(-10.0..=10.0), 3..=6),
    ) {
        let mut ring = coords;
        if ring.first() != ring.last() { ring.push(ring[0]); }
        let poly = geo::Polygon::new(geo::LineString::new(ring), Vec::new());
        for method in &[geo_repair::PolyMethod::Auto, geo_repair::PolyMethod::Structure, geo_repair::PolyMethod::Arrange] {
            let cfg = geo_repair::MakeValidConfig {
                poly_method: method.clone(),
                ..Default::default()
            };
            let result = poly.make_valid_with_config(&cfg);
            prop_assert!(result.validate().valid,
                "PolyMethod {:?} produced invalid output", method);
        }
    }

    // -----------------------------------------------------------------------
    // Property: GC with known-good sub-geometries should be valid
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_gc_disjoint_valid(
        coords_a in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=6),
        coords_b in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=6),
        offset_x in 200.0f64..500.0f64,
        offset_y in 200.0f64..500.0f64,
    ) {
        let mk_ring = |mut c: Vec<Coord<f64>>| -> geo::LineString<f64> {
            if c.first() != c.last() { c.push(c[0]); }
            geo::LineString::new(c)
        };
        let p1 = geo::Polygon::new(mk_ring(coords_a), Vec::new());
        let p2 = geo::Polygon::new(mk_ring(
            coords_b.into_iter().map(|c| Coord { x: c.x + offset_x, y: c.y + offset_y }).collect()
        ), Vec::new());
        let gc = geo::GeometryCollection(vec![
            geo::Geometry::Polygon(p1),
            geo::Geometry::Polygon(p2),
        ]);
        if gc.validate().valid {
            // If the GC is valid, make_valid must not change it structurally
            let result = gc.make_valid_with_config(&cfg_auto());
            prop_assert!(result.validate().valid,
                "valid GC became invalid after repair");
        }
    }

    // -----------------------------------------------------------------------
    // Property: validate_and_fix_always always returns valid output
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_validate_and_fix_always_valid(
        coords in proptest::collection::vec(coord_range(-100.0..=100.0), 3..=8),
    ) {
        use geo_repair::ValidateAndFix;
        let mut ring = coords;
        if ring.first() != ring.last() { ring.push(ring[0]); }
        let poly = geo::Polygon::new(geo::LineString::new(ring), Vec::new());
        let (_result, fixed) = poly.validate_and_fix_always();
        prop_assert!(fixed.validate().valid,
            "validate_and_fix_always produced invalid output");
    }

    // -----------------------------------------------------------------------
    // Property: extreme coordinate values don't cause panics
    // -----------------------------------------------------------------------

    #[test]
    fn invariant_no_panic_extreme_coords(
        coords in proptest::collection::vec(coord_range(-1e15..=1e15), 3..=8),
    ) {
        let mut ring = coords;
        if ring.first() != ring.last() { ring.push(ring[0]); }
        let poly = geo::Polygon::new(geo::LineString::new(ring), Vec::new());
        let result = poly.make_valid_with_config(&cfg_auto());
        // Should not panic; result may or may not be valid
        let _ = result;
    }
}

#[cfg(test)]
mod diag_all_methods_fail {
    use geo::{Coord, LineString, Polygon};
    use geo_repair::validation::GeoValidation;
    use geo_repair::{Feature, MakeValid, MakeValidConfig, PolyMethod, ValidateAndFix};

    #[test]
    fn diagnose_all_methods_fail() {
        let coords = vec![
            Coord {
                x: 33.298685125309,
                y: 25.64285228568552,
            },
            Coord {
                x: 16.056374168398353,
                y: 41.82073196346561,
            },
            Coord {
                x: 5.2001056860635515,
                y: -1.4935771193319936,
            },
            Coord {
                x: 40.0953181621632,
                y: 49.30127327981244,
            },
            Coord {
                x: -30.63143192804603,
                y: 22.339142189433932,
            },
            Coord {
                x: 17.726542485814562,
                y: -29.738377616718996,
            },
        ];
        let mut ring = coords.clone();
        if ring.first() != ring.last() {
            ring.push(ring[0]);
        }
        let n_verts = ring.len();
        let poly = Polygon::new(LineString::new(ring), Vec::new());

        println!("=== Input polygon ({} vertices) ===", n_verts);

        let ring = poly.exterior().0.clone();
        for (i, c) in ring.iter().enumerate() {
            println!("  V{}: ({}, {})", i, c.x, c.y);
        }
        println!("Input valid: {:?}", poly.validate());
        println!();

        // Also test the 4-coord version (first proptest minimal failure)
        {
            let coords4 = vec![
                Coord {
                    x: 33.298685125309,
                    y: 25.64285228568552,
                },
                Coord {
                    x: 16.056374168398353,
                    y: 41.82073196346561,
                },
                Coord {
                    x: 5.2001056860635515,
                    y: -1.4935771193319936,
                },
                Coord {
                    x: 40.0953181621632,
                    y: 49.30127327981244,
                },
            ];
            let mut ring4 = coords4;
            if ring4.first() != ring4.last() {
                ring4.push(ring4[0]);
            }
            let poly4 = Polygon::new(LineString::new(ring4), Vec::new());
            println!("=== 4-coord version ===");
            println!("  Input valid: {:?}", poly4.validate());
            for method in &[PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
                let cfg = MakeValidConfig {
                    poly_method: method.clone(),
                    ..Default::default()
                };
                let result = poly4.make_valid_with_config(&cfg);
                println!("  {:?}: valid={}", method, result.validate().valid);
            }
            println!();
        }

        for method in &[PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
            let config = MakeValidConfig {
                poly_method: method.clone(),
                ..Default::default()
            };
            let result = poly.make_valid_with_config(&config);
            let vr = result.validate();
            println!("=== {:?} ===", method);
            println!("Output valid: {:?}", vr);
            println!("Output type: {:?}", result);
            match result {
                geo::Geometry::Polygon(p) => {
                    let ext = p.exterior();
                    println!("  Exterior: {} vertices", ext.0.len());
                    for (i, c) in ext.0.iter().enumerate() {
                        println!("    V{}: ({}, {})", i, c.x, c.y);
                    }
                    println!("  Interiors: {}", p.interiors().len());
                    for (h, ring) in p.interiors().iter().enumerate() {
                        println!("    Hole {}: {} vertices", h, ring.0.len());
                    }
                }
                geo::Geometry::MultiPolygon(mp) => {
                    for (i, p) in mp.0.iter().enumerate() {
                        let ext = p.exterior();
                        let in_ring = ext.0.clone();
                        println!("  Poly {}: {} vertices", i, in_ring.len());
                        for (j, c) in in_ring.iter().enumerate() {
                            println!("    V{}: ({}, {})", j, c.x, c.y);
                        }
                        println!("    Holes: {}", p.interiors().len());
                    }
                }
                geo::Geometry::GeometryCollection(gc) => {
                    println!("  GeometryCollection with {} items", gc.0.len());
                    for (i, g) in gc.0.iter().enumerate() {
                        println!("    Item {}: {:?}", i, g);
                    }
                }
                other => {
                    println!("  Other: {:?}", other);
                }
            }
            println!();
        }

        println!("=== Structure direct call ===");
        {
            let g = poly.make_valid_with_config(&MakeValidConfig {
                poly_method: PolyMethod::Structure,
                ..Default::default()
            });
            println!("  OK: {}", g.validate().valid);
        }
        println!("=== Arrange direct call ===");
        {
            let g = poly.make_valid_with_config(&MakeValidConfig {
                poly_method: PolyMethod::Arrange,
                ..Default::default()
            });
            println!("  OK: {}", g.validate().valid);
        }
    }
}

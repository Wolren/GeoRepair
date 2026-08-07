//! Test battery.


use alloc::vec::Vec;
use super::*;


    #[test]
    fn test_point_valid() {
        assert!(Point::new(1.0, 2.0).is_valid());
    }

    #[test]
    fn test_point_nan() {
        // Point(NaN, NaN) is the geo representation of POINT EMPTY - valid OGC
        assert!(Point::new(f64::NAN, f64::NAN).is_valid());
        // A single NaN ordinate is invalid (GEOS TestValid: "P - invalid
        // NaN X ordinate" expects isValid=false; CoordinateNaN).
        assert!(!Point::new(f64::NAN, 2.0).is_valid());
        assert!(!Point::new(2.0, f64::NAN).is_valid());
        assert!(Point::new(1.0, 2.0).is_valid());
    }

    #[test]
    fn test_line_valid() {
        let l = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
        assert!(l.is_valid());
    }

    #[test]
    fn test_line_degenerate() {
        let l = Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 });
        assert!(!l.is_valid());
        assert!(l.validate_reason().contains("zero length"));
    }

    #[test]
    fn test_linestring_valid() {
        let ls = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        ]);
        assert!(ls.is_valid());
    }

    #[test]
    fn test_linestring_empty() {
        let ls = LineString::<f64>::new(Vec::new());
        assert!(!ls.is_valid());
    }

    #[test]
    fn test_polygon_valid() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(poly.is_valid());
    }

    #[test]
    fn test_ring_not_closed() {
        let ring = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
        ]);
        let result = check_ring_validity(&ring.0, true);
        assert!(!result.is_empty());
        assert!(result
            .iter()
            .any(|e| matches!(e, GeometryValidationError::RingNotClosed { .. })));
    }

    #[test]
    fn test_polygon_too_few_points() {
        let poly = Polygon::new(
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
            Vec::new(),
        );
        assert!(!poly.is_valid());
    }

    #[test]
    fn test_bowtie_self_intersects() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(!poly.is_valid());
        assert_eq!(
            poly.validate().errors[0],
            GeometryValidationError::SelfIntersection
        );
    }

    #[test]
    fn test_polygon_with_hole() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 20.0, y: 0.0 },
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 0.0, y: 20.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 5.0, y: 15.0 },
                Coord { x: 15.0, y: 15.0 },
                Coord { x: 15.0, y: 5.0 },
            ])],
        );
        assert!(poly.is_valid());
    }

    #[test]
    fn test_hole_outside_shell() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 25.0, y: 20.0 },
                Coord { x: 25.0, y: 25.0 },
                Coord { x: 20.0, y: 25.0 },
                Coord { x: 20.0, y: 20.0 },
            ])],
        );
        assert!(!poly.is_valid());
    }

    #[test]
    fn test_validate_reason_valid() {
        let p = Point::new(1.0, 2.0);
        assert_eq!(p.validate_reason(), "Valid Geometry");
    }

    #[test]
    fn test_multipolygon_overlapping() {
        let mp = MultiPolygon::new(vec![
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 0.0 },
                    Coord { x: 5.0, y: 0.0 },
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 0.0, y: 5.0 },
                    Coord { x: 0.0, y: 0.0 },
                ]),
                Vec::new(),
            ),
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 2.0, y: 2.0 },
                    Coord { x: 7.0, y: 2.0 },
                    Coord { x: 7.0, y: 7.0 },
                    Coord { x: 2.0, y: 7.0 },
                    Coord { x: 2.0, y: 2.0 },
                ]),
                Vec::new(),
            ),
        ]);
        assert!(!mp.is_valid());
    }

    #[test]
    fn test_multipolygon_shells_cross() {
        // Two shells that cross (neither contains the other's first point)
        let mp = MultiPolygon::new(vec![
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 3.0 },
                    Coord { x: 10.0, y: 3.0 },
                    Coord { x: 10.0, y: 5.0 },
                    Coord { x: 0.0, y: 5.0 },
                    Coord { x: 0.0, y: 3.0 },
                ]),
                Vec::new(),
            ),
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 4.0, y: 0.0 },
                    Coord { x: 6.0, y: 0.0 },
                    Coord { x: 6.0, y: 8.0 },
                    Coord { x: 4.0, y: 8.0 },
                    Coord { x: 4.0, y: 0.0 },
                ]),
                Vec::new(),
            ),
        ]);
        assert!(!mp.is_valid());
    }

    #[test]
    fn test_triangle_valid() {
        let t = Triangle::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 5.0, y: 0.0 },
            Coord { x: 2.5, y: 5.0 },
        );
        assert!(t.is_valid());
    }

    #[test]
    fn test_triangle_degenerate() {
        let t = Triangle::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        );
        assert!(!t.is_valid());
    }

    #[test]
    fn test_rect_valid() {
        let r = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert!(r.is_valid());
    }

    #[test]
    fn test_rect_nan() {
        let r = Rect::new(Point::new(f64::NAN, 0.0), Point::new(10.0, 10.0));
        assert!(!r.is_valid());
    }

    #[test]
    fn test_geometry_dispatch() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        assert!(g.is_valid());

        // Single NaN ordinate: invalid (GEOS parity, CoordinateNaN).
        let g2 = Geometry::Point(Point::new(f64::NAN, 2.0));
        assert!(!g2.is_valid());
    }

    #[test]
    fn test_geometry_collection() {
        // Empty point (NaN, NaN) is valid; a NaN ordinate is not.
        let gc = GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::Point(Point::new(f64::NAN, f64::NAN)),
        ]);
        assert!(gc.is_valid());
        assert_eq!(gc.validate().errors.len(), 0);

        let gc2 = GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::Point(Point::new(f64::NAN, 2.0)),
        ]);
        assert!(!gc2.is_valid());
    }

    #[test]
    fn test_multilinestring_not_simple() {
        let mls = MultiLineString::new(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 }]),
            LineString::new(vec![Coord { x: 0.0, y: 10.0 }, Coord { x: 10.0, y: 0.0 }]),
        ]);
        let result = mls.validate();
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::NotSimple)));
    }

    #[test]
    fn test_multilinestring_simple() {
        let mls = MultiLineString::new(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 0.0 }]),
            LineString::new(vec![Coord { x: 0.0, y: 10.0 }, Coord { x: 10.0, y: 10.0 }]),
        ]);
        assert!(mls.is_valid());
    }

    #[test]
    fn test_linestring_zero_length() {
        let ls = LineString::new(vec![Coord { x: 1.0, y: 2.0 }, Coord { x: 1.0, y: 2.0 }]);
        assert!(!ls.is_valid());
    }

    #[test]
    fn test_linestring_zero_length_many_coords() {
        let ls = LineString::new(vec![
            Coord { x: 3.0, y: 4.0 },
            Coord { x: 3.0, y: 4.0 },
            Coord { x: 3.0, y: 4.0 },
            Coord { x: 3.0, y: 4.0 },
        ]);
        assert!(!ls.is_valid());
    }

    #[test]
    fn test_hole_edges_cross_shell() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 12.0, y: 5.0 },
                Coord { x: 12.0, y: 8.0 },
                Coord { x: 5.0, y: 8.0 },
                Coord { x: 5.0, y: 5.0 },
            ])],
        );
        assert!(!poly.is_valid());
    }

    #[test]
    fn test_excessive_nesting() {
        let inner = Geometry::Point(Point::new(1.0, 2.0));
        let mut gc = GeometryCollection(vec![inner]);
        for _ in 0..150 {
            gc = GeometryCollection(vec![Geometry::GeometryCollection(gc)]);
        }
        assert!(!gc.is_valid());
        assert!(gc
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::ExcessiveNesting)));
    }

    #[test]
    fn test_pinch_point() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let result = poly.validate();
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::PinchPoint)));
    }

    #[test]
    fn test_nested_holes() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 50.0, y: 0.0 },
                Coord { x: 50.0, y: 50.0 },
                Coord { x: 0.0, y: 50.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![
                LineString::new(vec![
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 45.0, y: 5.0 },
                    Coord { x: 45.0, y: 45.0 },
                    Coord { x: 5.0, y: 45.0 },
                    Coord { x: 5.0, y: 5.0 },
                ]),
                LineString::new(vec![
                    Coord { x: 10.0, y: 10.0 },
                    Coord { x: 20.0, y: 10.0 },
                    Coord { x: 20.0, y: 20.0 },
                    Coord { x: 10.0, y: 20.0 },
                    Coord { x: 10.0, y: 10.0 },
                ]),
            ],
        );
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::NestedHoles)));
    }

    #[test]
    fn test_disconnected_interior_ring() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 50.0, y: 0.0 },
                Coord { x: 50.0, y: 50.0 },
                Coord { x: 0.0, y: 50.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![
                LineString::new(vec![
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 45.0, y: 5.0 },
                    Coord { x: 45.0, y: 45.0 },
                    Coord { x: 5.0, y: 45.0 },
                    Coord { x: 5.0, y: 5.0 },
                ]),
                LineString::new(vec![
                    Coord { x: 8.0, y: 8.0 },
                    Coord { x: 12.0, y: 8.0 },
                    Coord { x: 12.0, y: 12.0 },
                    Coord { x: 8.0, y: 12.0 },
                    Coord { x: 8.0, y: 8.0 },
                ]),
            ],
        );
        let errors = &poly.validate().errors;
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, GeometryValidationError::NestedHoles))
                || errors
                    .iter()
                    .any(|e| matches!(e, GeometryValidationError::DisconnectedInteriorRing)),
            "expected NestedHoles or DisconnectedInteriorRing, got: {errors:?}",
        );
    }

    #[test]
    fn test_wrong_orientation_shell_cw() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::WrongOrientation)));
    }

    #[test]
    fn test_repeated_point_in_ring() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::RepeatedPoint)));
    }

    #[test]
    fn test_duplicated_rings() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 20.0, y: 0.0 },
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 0.0, y: 20.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![
                LineString::new(vec![
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 5.0, y: 15.0 },
                    Coord { x: 15.0, y: 15.0 },
                    Coord { x: 15.0, y: 5.0 },
                    Coord { x: 5.0, y: 5.0 },
                ]),
                LineString::new(vec![
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 5.0, y: 15.0 },
                    Coord { x: 15.0, y: 15.0 },
                    Coord { x: 15.0, y: 5.0 },
                    Coord { x: 5.0, y: 5.0 },
                ]),
            ],
        );
        assert!(!poly.is_valid());
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::DuplicatedRings)));
    }

    #[test]
    fn test_multipoint_duplicate_points() {
        let mp = MultiPoint::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
            Point::new(1.0, 2.0),
        ]);
        assert!(!mp.is_valid());
        assert!(mp
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::MultiPointDuplicatePoints)));
    }

    #[test]
    fn test_multilinestring_duplicate_lines() {
        let mls = MultiLineString::new(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 }]),
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 }]),
        ]);
        assert!(!mls.is_valid());
        assert!(mls
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::MultiLineStringDuplicateLines)));
    }

    #[test]
    fn test_degenerate_exterior_collinear_x() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 20.0, y: 0.0 },
                Coord { x: 30.0, y: 0.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(!poly.is_valid());
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::DegenerateExterior)));
    }

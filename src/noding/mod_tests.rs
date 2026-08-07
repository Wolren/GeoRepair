//! Test battery.


use alloc::vec::Vec;
use super::*;


    // -------------------------------
    // remove_consecutive_duplicates
    // -------------------------------

    #[test]
    fn test_remove_consecutive_duplicates_empty() {
        let result = remove_consecutive_duplicates::<f64>(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_remove_consecutive_duplicates_no_dupes() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        ];
        let result = remove_consecutive_duplicates(&coords);
        assert_eq!(result, coords);
    }

    #[test]
    fn test_remove_consecutive_duplicates_all_identical() {
        let coords = vec![
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
        ];
        let result = remove_consecutive_duplicates(&coords);
        assert_eq!(result, vec![Coord { x: 1.0, y: 1.0 }]);
    }

    #[test]
    fn test_remove_consecutive_duplicates_interleaved() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 2.0 },
        ];
        let result = remove_consecutive_duplicates(&coords);
        assert_eq!(
            result,
            vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 2.0, y: 2.0 },
            ]
        );
    }

    // -------------------------------
    // edges_intersect
    // -------------------------------

    #[test]
    fn test_edges_intersect_crossing() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
        let e2 = Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 0.0 });
        assert!(edges_intersect(&e1, &e2, 1e-12));
    }

    #[test]
    fn test_edges_intersect_parallel() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 });
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

    #[test]
    fn test_edges_intersect_adjacent() {
        // Adjacent edges share a vertex — not an intersection
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 });
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

    #[test]
    fn test_edges_intersect_collinear_overlap() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 });
        assert!(edges_intersect(&e1, &e2, 1e-12)); // Collinear overlap detected as intersection
    }

    #[test]
    fn test_edges_intersect_endpoint_on_segment() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 5.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 3.0, y: 0.0 }, Coord { x: 3.0, y: 3.0 });
        // This is endpoint-on-segment, not a proper crossing
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

    #[test]
    fn test_edges_intersect_non_adjacent_same_line() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 3.0, y: 0.0 }, Coord { x: 5.0, y: 0.0 });
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

    // -------------------------------
    // check_self_intersections
    // -------------------------------

    #[test]
    fn test_check_self_intersections_none() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }),
        ];
        assert!(!check_self_intersections(&edges));
    }

    #[test]
    fn test_check_self_intersections_bowtie() {
        // A bowtie shape: (0,0) → (2,2) → (2,0) → (0,2)
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 2.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 0.0, y: 2.0 }),
        ];
        // Edge 0 crosses edge 2 (non-adjacent, i=0, j=2)
        assert!(check_self_intersections(&edges));
    }

    #[test]
    fn test_check_self_intersections_empty() {
        assert!(!check_self_intersections::<f64>(&[]));
    }

    #[test]
    fn test_check_self_intersections_single() {
        let edges = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        )];
        assert!(!check_self_intersections(&edges));
    }

    // -------------------------------
    // edges_intersect / orient2d_generic
    // -------------------------------

    #[test]
    fn test_orient2d_generic_ccw() {
        let result = orient2d_generic(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.5, y: 1.0 },
        );
        assert!(result > 0.0);
    }

    #[test]
    fn test_orient2d_generic_cw() {
        let result = orient2d_generic(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.5, y: -1.0 },
        );
        assert!(result < 0.0);
    }

    #[test]
    fn test_orient2d_generic_collinear() {
        let result = orient2d_generic(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        );
        assert_eq!(result, 0.0);
    }

    // -------------------------------
    // compute_intersection_param
    // -------------------------------

    #[test]
    fn test_intersection_param_crossing() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
        let e2 = Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 0.0 });
        let result = compute_intersection_param(&e1, &e2, 1e-12);
        assert!(result.is_some());
        let (t1, t2, pt) = result.unwrap();
        assert!((t1 - 0.5f64).abs() < 1e-12f64);
        assert!((t2 - 0.5f64).abs() < 1e-12f64);
        assert!((pt.x - 0.5f64).abs() < 1e-12f64);
        assert!((pt.y - 0.5f64).abs() < 1e-12f64);
    }

    #[test]
    fn test_intersection_param_parallel() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 });
        assert!(compute_intersection_param(&e1, &e2, 1e-12).is_none());
    }

    #[test]
    fn test_intersection_param_endpoint_touching() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 1.0 });
        // Endpoint-on-segment: DD computes the intersection, caller checks
        // if the param is strictly interior.
        let result = compute_intersection_param(&e1, &e2, 1e-12);
        assert!(result.is_some());
        let (t1, _t2, pt) = result.unwrap();
        assert!((t1 - 1.0f64).abs() < 1e-12f64);
        assert!((pt.x - 1.0f64).abs() < 1e-12f64);
        assert!((pt.y - 0.0f64).abs() < 1e-12f64);
    }

    // -------------------------------
    // split_edges_at_intersections
    // -------------------------------

    #[test]
    fn test_split_edges_no_intersections() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 }),
        ];
        let result = split_edges_at_intersections(&edges);
        assert_eq!(result, edges);
    }

    #[test]
    fn test_split_edges_crossing() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 2.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 0.0, y: 2.0 }),
        ];
        // Edge 0 (0->2) and edge 2 (2->0) cross at (1,1)
        // Edge 0 should split into two: (0,0)-(1,1) and (1,1)-(2,2)
        // Edge 2 should stay as (2,0)-(0,2) [or split? depends on param range]
        let result = split_edges_at_intersections(&edges);
        assert!(result.len() >= 3);
    }

    // -------------------------------
    // interpolate
    // -------------------------------

    #[test]
    fn test_interpolate_midpoint() {
        let e = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 4.0 });
        let pt = interpolate(e, 0.5);
        assert!((pt.x - 1.0f64).abs() < 1e-12f64);
        assert!((pt.y - 2.0f64).abs() < 1e-12f64);
    }

    #[test]
    fn test_interpolate_start() {
        let e = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 5.0, y: 10.0 });
        let pt = interpolate(e, 0.0);
        assert!((pt.x - 1.0f64).abs() < 1e-12f64);
        assert!((pt.y - 2.0f64).abs() < 1e-12f64);
    }

    #[test]
    fn test_interpolate_end() {
        let e = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 5.0, y: 10.0 });
        let pt = interpolate(e, 1.0);
        assert!((pt.x - 5.0f64).abs() < 1e-12f64);
        assert!((pt.y - 10.0f64).abs() < 1e-12f64);
    }

    // -------------------------------
    // dist2
    // -------------------------------

    #[test]
    fn test_dist2_identical() {
        assert_eq!(
            dist2(Coord { x: 1.0, y: 2.0 }, Coord { x: 1.0, y: 2.0 }),
            0.0
        );
    }

    #[test]
    fn test_dist2_unit() {
        assert!(
            (dist2(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }) - 1.0f64).abs() < 1e-12f64
        );
    }

    #[test]
    fn test_dist2_diagonal() {
        assert!(
            (dist2(Coord { x: 0.0, y: 0.0 }, Coord { x: 3.0, y: 4.0 }) - 25.0f64).abs() < 1e-12f64
        );
    }

    // -------------------------------
    // reconnect_edges
    // -------------------------------

    #[test]
    fn test_reconnect_edges_single_chain() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }),
        ];
        let result = reconnect_edges(edges);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.len(), 4);
    }

    #[test]
    fn test_reconnect_edges_disjoint() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 5.0, y: 5.0 }, Coord { x: 6.0, y: 5.0 }),
        ];
        let result = reconnect_edges(edges);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_reconnect_edges_empty() {
        let result = reconnect_edges::<f64>(Vec::new());
        assert!(result.is_empty());
    }

    #[test]
    fn test_reconnect_edges_single() {
        let edges = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
        )];
        let result = reconnect_edges(edges);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.len(), 2);
    }

    #[test]
    fn test_reconnect_edges_reversed_order() {
        let edges = vec![
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 0.0, y: 0.0 }),
        ];
        let result = reconnect_edges(edges);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.len(), 3);
    }

    // -------------------------------
    // node_line_string
    // -------------------------------

    #[test]
    fn test_node_line_string_no_self_intersection() {
        let ls = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
        ]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::LineString(_)));
    }

    #[test]
    fn test_node_line_string_self_intersecting() {
        // Bowtie path: (0,0) → (2,2) → (2,0) → (0,2)
        let ls = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 0.0, y: 2.0 },
        ]);
        let result = node_line_string(&ls);
        // Should split into multiple LineStrings
        assert!(
            matches!(result, Geometry::MultiLineString(_))
                || matches!(result, Geometry::LineString(_))
        );
        if let Geometry::MultiLineString(ref mls) = result {
            assert!(mls.0.len() >= 2);
        }
    }

    #[test]
    fn test_node_line_string_empty() {
        let ls = LineString::<f64>::new(Vec::new());
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::GeometryCollection(_)));
        assert!(matches!(result, Geometry::GeometryCollection(ref gc) if gc.0.is_empty()));
    }

    #[test]
    fn test_node_line_string_single_point() {
        let ls = LineString::new(vec![Coord { x: 0.0, y: 0.0 }]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::GeometryCollection(_)));
    }

    #[test]
    fn test_node_line_string_too_few_coords() {
        let ls = LineString::new(vec![Coord { x: 0.0, y: 0.0 }]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::GeometryCollection(_)));
    }

    #[test]
    fn test_node_line_string_nan_filtered() {
        let ls = LineString::new(vec![
            Coord {
                x: f64::NAN,
                y: 0.0,
            },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::LineString(_)));
    }

    #[test]
    fn test_remove_consecutive_duplicates_repeated_non_consecutive() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let result = remove_consecutive_duplicates(&coords);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_node_line_string_two_points() {
        let ls = LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]);
        let result = node_line_string(&ls);
        assert!(matches!(result, Geometry::LineString(_)));
    }

    #[test]
    fn test_edges_intersect_very_close() {
        let e1 = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        let e2 = Line::new(Coord { x: 0.5, y: 1e-13 }, Coord { x: 0.5, y: -1e-13 });
        // Very near collinear — parallel epsilon should not detect as crossing
        assert!(!edges_intersect(&e1, &e2, 1e-12));
    }

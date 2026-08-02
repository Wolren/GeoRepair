//! Test battery.

use super::*;

    use geo::Polygon;
    use crate::structure::fix_ring::{
        basic_cleanup, edges_from_coords, has_self_intersections, repair_ring, split_edges,
    };

    fn ring_area(ring: &LineString<f64>) -> f64 {
        let mut a = 0.0;
        for w in ring.0.windows(2) {
            a += w[0].x * w[1].y - w[1].x * w[0].y;
        }
        a.abs() * 0.5
    }

    fn total_area(rings: &[LineString<f64>]) -> f64 {
        rings.iter().map(ring_area).sum()
    }

    fn poly_total_area(polys: &[Polygon<f64>]) -> f64 {
        use geo::Area;
        polys.iter().map(|p| p.unsigned_area()).sum()
    }

    #[test]
    fn test_square() {
        let ring = ls(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        let input_area = ring_area(&ring);
        let r = repair_ring(&ring);
        assert!(r.is_some());
        let rings = r.unwrap();
        assert_eq!(rings.len(), 1);
        let output_area = poly_total_area(&rings);
        if input_area > 0.0 {
            assert!(
                (output_area / input_area - 1.0).abs() < 0.5,
                "square area ratio {:.4}",
                output_area / input_area
            );
        }
    }

    #[test]
    fn test_bowtie() {
        let ring = ls(&[
            (0.0, 0.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        let input_area = ring_area(&ring);
        let r = repair_ring(&ring);
        assert!(r.is_some(), "bowtie should produce result");
        let rings = r.unwrap();
        assert!(!rings.is_empty(), "bowtie should produce at least one ring");
        for ring in &rings {
            assert!(ring.exterior().0.len() >= 4, "ring too short");
            assert_eq!(
                ring.exterior().0.first(),
                ring.exterior().0.last(),
                "ring not closed"
            );
        }
        let output_area = poly_total_area(&rings);
        if input_area > 0.0 {
            assert!(
                (output_area / input_area - 1.0).abs() < 0.5,
                "bowtie area ratio {:.4}",
                output_area / input_area
            );
        } else {
            assert!(
                output_area > 0.0,
                "bowtie output should have positive area, got {:.0}",
                output_area
            );
        }
    }

    #[test]
    fn test_empty() {
        let ring = LineString::<f64>::new(Vec::new());
        assert!(repair_ring(&ring).is_none());
    }

    #[test]
    fn test_square_two_faces() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
            Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 0.0, y: 1.0 }),
            Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 0.0, y: 0.0 }),
        ];
        let graph = build_graph(&edges);
        let faces = extract_all_faces(&graph);
        assert!(faces.is_some());
        assert_eq!(faces.unwrap().len(), 2);
    }

    #[test]
    fn test_has_self_intersections_true() {
        let coords = coords(&[
            (0.0, 0.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        assert!(has_self_intersections(&coords));
    }

    #[test]
    fn test_has_self_intersections_false() {
        let coords = coords(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        assert!(!has_self_intersections(&coords));
    }

    #[test]
    fn test_split_edges_crossing() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 2.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 0.0, y: 2.0 }),
        ];
        let result = split_edges(&edges);
        assert!(
            result.len() >= 4,
            "crossing edges should split: got {}",
            result.len()
        );
    }

    #[test]
    fn test_three_lobes() {
        let ring = ls(&[
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 0.0),
        ]);
        let r = repair_ring(&ring);
        assert!(r.is_some());
        let rings = r.unwrap();
        assert!(!rings.is_empty());
        for ring in &rings {
            assert!(ring.exterior().0.len() >= 4);
            assert_eq!(ring.exterior().0.first(), ring.exterior().0.last());
        }
    }

    #[test]
    fn test_large_coords() {
        let ring = ls(&[
            (0.0, 0.0),
            (1_000_000.0, 1_000_000.0),
            (1_000_000.0, 0.0),
            (0.0, 1_000_000.0),
            (0.0, 0.0),
        ]);
        let r = repair_ring(&ring);
        assert!(r.is_some());
    }

    #[test]
    fn diagnose_fuzz_failure() {
        let ring = ls(&[
            (-32.94925304356217, -37.4509724868373),
            (25.087850997208253, -29.87382634047737),
            (0.0, -48.64262720158944),
            (-40.61251938421724, -45.1172049629247),
            (-38.51974407936723, -13.433918287897887),
            (-16.8110711840133, -46.226614473001),
        ]);
        let coords = basic_cleanup(&ring).unwrap();
        eprintln!("after cleanup: {} coords", coords.len());
        for (i, c) in coords.iter().enumerate() {
            eprintln!("  coords[{}]: ({}, {})", i, c.x, c.y);
        }
        let si = has_self_intersections(&coords);
        eprintln!("has_self_intersections: {}", si);
        if !si {
            return;
        }

        let edges = edges_from_coords(&coords);
        eprintln!("edges: {}", edges.len());
        let noded = split_edges(&edges);
        eprintln!("noded edges: {}", noded.len());
        for (i, e) in noded.iter().enumerate() {
            eprintln!(
                "  e[{}]: ({},{}) -> ({},{})",
                i, e.start.x, e.start.y, e.end.x, e.end.y
            );
        }
        let graph = build_graph(&noded);
        eprintln!(
            "graph: {} verts, {} edges",
            graph.verts.len(),
            graph.edges.len()
        );
        for (i, v) in graph.verts.iter().enumerate() {
            eprintln!("  v[{}]: ({}, {})", i, v.x, v.y);
        }
        for (i, (fi, ti)) in graph.edges.iter().enumerate() {
            eprintln!("  edge[{}]: {} -> {}", i, fi, ti);
        }

        let faces = extract_all_faces(&graph).unwrap();
        eprintln!("extracted {} faces", faces.len());
        for (fi, face) in faces.iter().enumerate() {
            eprintln!("  face[{}]: {} edges", fi, face.len());
            for &(ei, to) in face {
                eprint!(" (e{},v{})", ei, to);
            }
            eprintln!();
            let mut ring: Vec<Coord<f64>> = face.iter().map(|&(_, to)| graph.verts[to]).collect();
            if ring.len() >= 3 {
                ring.push(ring[0]);
            }
            let check_si = has_self_intersections(&ring);
            eprintln!("    self-intersecting boundary: {}", check_si);
        }

        let simple_faces: Vec<Vec<(usize, usize)>> = faces
            .iter()
            .flat_map(|f| split_face_at_pinch_points(f, &graph.edges))
            .filter(|f| f.len() >= 3)
            .collect();
        eprintln!("after pinch-split: {} simple faces", simple_faces.len());
        for (fi, face) in simple_faces.iter().enumerate() {
            eprintln!("  simple_face[{}]: {} edges", fi, face.len());
            let mut ring: Vec<Coord<f64>> = face.iter().map(|&(_, to)| graph.verts[to]).collect();
            if ring.len() >= 3 {
                ring.push(ring[0]);
            }
            let check_si = has_self_intersections(&ring);
            eprintln!("    self-intersecting boundary: {}", check_si);
        }

        let interior =
            label_interior_faces(&noded, &graph.verts, &coords, &simple_faces, &graph.edges)
                .unwrap();
        eprintln!("interior faces: {:?}", interior);
        for &fi in &interior {
            let face = &simple_faces[fi];
            let mut ring_coords: Vec<Coord<f64>> = face
                .iter()
                .map(|&(_, to_idx)| graph.verts[to_idx])
                .collect();
            eprintln!("  interior face[{}]: {} coords", fi, ring_coords.len());
            if ring_coords.len() >= 3 {
                ring_coords.push(ring_coords[0]);
            }
            let check_si = has_self_intersections(&ring_coords);
            eprintln!("    self-intersecting: {}", check_si);
            for (i, c) in ring_coords.iter().enumerate() {
                eprintln!("    ring[{}]: ({}, {})", i, c.x, c.y);
            }
        }
    }

    fn ls(pairs: &[(f64, f64)]) -> LineString<f64> {
        LineString::new(pairs.iter().map(|&(x, y)| Coord { x, y }).collect())
    }

    fn coords(pairs: &[(f64, f64)]) -> Vec<Coord<f64>> {
        pairs.iter().map(|&(x, y)| Coord { x, y }).collect()
    }

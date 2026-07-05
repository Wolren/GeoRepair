#[cfg(test)]
mod tests {
    use geo_repair::structure::fix_ring_graph::{build_graph, extract_all_faces_geos};
    use geo::{Coord, Line};

    #[test]
    fn diagnose_deep_nesting() {
        let lines = vec![
            // L0 outer: CCW
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 20.0, y: 0.0 }),
            Line::new(Coord { x: 20.0, y: 0.0 }, Coord { x: 20.0, y: 20.0 }),
            Line::new(Coord { x: 20.0, y: 20.0 }, Coord { x: 0.0, y: 20.0 }),
            Line::new(Coord { x: 0.0, y: 20.0 }, Coord { x: 0.0, y: 0.0 }),
            // L1 CW hole
            Line::new(Coord { x: 3.0, y: 3.0 }, Coord { x: 17.0, y: 3.0 }),
            Line::new(Coord { x: 17.0, y: 3.0 }, Coord { x: 17.0, y: 17.0 }),
            Line::new(Coord { x: 17.0, y: 17.0 }, Coord { x: 3.0, y: 17.0 }),
            Line::new(Coord { x: 3.0, y: 17.0 }, Coord { x: 3.0, y: 3.0 }),
            // L2 CCW island
            Line::new(Coord { x: 6.0, y: 6.0 }, Coord { x: 14.0, y: 6.0 }),
            Line::new(Coord { x: 14.0, y: 6.0 }, Coord { x: 14.0, y: 14.0 }),
            Line::new(Coord { x: 14.0, y: 14.0 }, Coord { x: 6.0, y: 14.0 }),
            Line::new(Coord { x: 6.0, y: 14.0 }, Coord { x: 6.0, y: 6.0 }),
        ];
        let graph = build_graph(&lines);
        eprintln!("verts: {:?}", graph.verts);
        eprintln!("edges: {:?}", graph.edges);
        for (v, adj) in graph.sorted_adj.iter().enumerate() {
            eprintln!("  adj[{}]: {:?}", v, adj);
        }
        let faces = extract_all_faces_geos(&graph);
        eprintln!("faces count: {}", faces.as_ref().map(|f| f.len()).unwrap_or(0));
        if let Some(ref f) = faces {
            for (i, face) in f.iter().enumerate() {
                eprintln!("  face {} ({} edges):", i, face.len());
                for &(ei, to) in face {
                    eprintln!("    ei={} to={}", ei, to);
                }
            }
        }
        // This test is for diagnostics only
        assert!(true);
    }
}

use rustc_hash::FxHashSet;

use geo::Coord;
use spade::handles::{FixedDirectedEdgeHandle, FixedFaceHandle, InnerTag};
use spade::{ConstrainedDelaunayTriangulation, Triangulation};

pub(crate) fn trace_rings(
    cdt: &ConstrainedDelaunayTriangulation<Coord<f64>>,
    interior: &FxHashSet<FixedFaceHandle<InnerTag>>,
) -> Vec<Vec<Coord<f64>>> {
    let is_interior = |face: spade::handles::FaceHandle<
        '_,
        spade::handles::PossiblyOuterTag,
        Coord<f64>,
        _,
        _,
        _,
    >|
     -> bool {
        face.as_inner()
            .map(|f| interior.contains(&f.fix()))
            .unwrap_or(false)
    };

    let boundary_edges: FxHashSet<FixedDirectedEdgeHandle> = cdt
        .directed_edges()
        .filter(|e| is_interior(e.face()) && !is_interior(e.rev().face()))
        .map(|e| e.fix())
        .collect();

    let mut used: FxHashSet<FixedDirectedEdgeHandle> = FxHashSet::default();
    let mut rings = Vec::new();

    for &start_handle in &boundary_edges {
        if used.contains(&start_handle) {
            continue;
        }

        let mut coords: Vec<Coord<f64>> = Vec::new();
        let mut current_fix = start_handle;
        let max_edges = boundary_edges.len();
        let mut outer_safety = max_edges;

        loop {
            used.insert(current_fix);
            let current = cdt.directed_edge(current_fix);
            let pos = current.from().position();
            coords.push(Coord { x: pos.x, y: pos.y });

            let mut candidate = current.next();
            let mut inner_safety = cdt.num_directed_edges();
            loop {
                if boundary_edges.contains(&candidate.fix()) && !used.contains(&candidate.fix()) {
                    break;
                }
                if candidate.fix() == start_handle {
                    break;
                }
                candidate = candidate.rev().next();
                inner_safety = inner_safety.saturating_sub(1);
                if inner_safety == 0 {
                    break;
                }
            }

            current_fix = candidate.fix();
            if current_fix == start_handle {
                break;
            }
            outer_safety = outer_safety.saturating_sub(1);
            if outer_safety == 0 {
                break;
            }
        }

        if let Some(&first) = coords.first() {
            coords.push(first);
        }
        if coords.len() >= 4 {
            rings.push(coords);
        }
    }

    rings
}

pub(crate) fn split_ring_at_pinch_points(mut coords: Vec<Coord<f64>>) -> Vec<Vec<Coord<f64>>> {
    if coords.len() >= 2 && coords.first() == coords.last() {
        coords.pop();
    }
    if coords.len() < 3 {
        return Vec::new();
    }

    let mut result: Vec<Vec<Coord<f64>>> = Vec::new();
    loop {
        let repeat = coords.iter().enumerate().find_map(|(i, c)| {
            coords[i + 1..]
                .iter()
                .position(|other| other == c)
                .map(|j| (i, i + 1 + j))
        });
        match repeat {
            Some((first, second)) => {
                let sub: Vec<Coord<f64>> = coords[first..=second].to_vec();
                coords.drain((first + 1)..=second);
                if sub.len() >= 4 {
                    result.extend(split_ring_at_pinch_points(sub));
                }
            }
            None => break,
        }
    }

    if coords.len() >= 3 {
        let first = coords[0];
        coords.push(first);
        result.push(coords);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_ring_at_pinch_points_none() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let result = split_ring_at_pinch_points(coords);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_split_ring_at_pinch_points_one_pinch() {
        // A figure-8 shape with a single pinch point
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let result = split_ring_at_pinch_points(coords);
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_split_ring_at_pinch_points_empty() {
        let result = split_ring_at_pinch_points(Vec::new());
        assert!(result.is_empty());
    }

    #[test]
    fn test_split_ring_at_pinch_points_fewer_than_3() {
        let result =
            split_ring_at_pinch_points(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_split_ring_at_pinch_points_already_split() {
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let result = split_ring_at_pinch_points(coords);
        assert_eq!(result.len(), 1);
        assert!(result[0].len() >= 4);
    }
}

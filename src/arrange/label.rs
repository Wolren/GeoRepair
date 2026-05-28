use std::collections::{HashSet, VecDeque};

use geo::Coord;
use spade::handles::{FixedFaceHandle, InnerTag};
use spade::{ConstrainedDelaunayTriangulation, Triangulation};

pub(crate) fn label_faces(
    cdt: &ConstrainedDelaunayTriangulation<Coord<f64>>,
) -> HashSet<FixedFaceHandle<InnerTag>> {
    let mut interior: HashSet<FixedFaceHandle<InnerTag>> = HashSet::new();
    let mut visited: HashSet<FixedFaceHandle<InnerTag>> = HashSet::new();
    let mut queue: VecDeque<(FixedFaceHandle<InnerTag>, bool)> = VecDeque::new();

    for edge in cdt.directed_edges() {
        if !edge.face().is_outer() {
            continue;
        }
        let opposite = edge.rev().face();
        if let Some(inner) = opposite.as_inner() {
            let handle = inner.fix();
            if visited.insert(handle) {
                let is_interior = edge.is_constraint_edge();
                if is_interior {
                    interior.insert(handle);
                }
                queue.push_back((handle, is_interior));
            }
        }
    }

    while let Some((face_handle, face_is_interior)) = queue.pop_front() {
        let face = cdt.face(face_handle);
        for edge in face.adjacent_edges() {
            let neighbour = edge.rev().face();
            if let Some(inner) = neighbour.as_inner() {
                let n_handle = inner.fix();
                if visited.insert(n_handle) {
                    let crosses = edge.is_constraint_edge();
                    let n_interior = if crosses {
                        !face_is_interior
                    } else {
                        face_is_interior
                    };
                    if n_interior {
                        interior.insert(n_handle);
                    }
                    queue.push_back((n_handle, n_interior));
                }
            }
        }
    }

    interior
}

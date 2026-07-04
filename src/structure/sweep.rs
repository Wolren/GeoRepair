use geo::Coord;
use rstar::{AABB, RTree, RTreeObject};

use crate::orient::orient2d_fast;

/// Edge with 2D bounding envelope for R-tree spatial indexing.
struct EdgeEnvelope {
    index: u32,
    envelope: AABB<[f64; 2]>,
}

impl RTreeObject for EdgeEnvelope {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

/// Build bounding envelope for edge i, expanded so that nearly-vertical edges
/// and near-touching intervals are still detected.
fn edge_envelope(coords: &[Coord<f64>], i: usize) -> AABB<[f64; 2]> {
    let lo_x = coords[i].x.min(coords[i + 1].x);
    let hi_x = coords[i].x.max(coords[i + 1].x);
    let lo_y = coords[i].y.min(coords[i + 1].y);
    let hi_y = coords[i].y.max(coords[i + 1].y);
    let ext = f64::EPSILON * 100.0 * (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0);
    AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext])
}

/// Self-intersection detection using an R-tree over edge bounding boxes.
///
/// Builds an `rstar::RTree` keyed by each edge's 2D bounding envelope, then
/// queries each edge against the tree to find candidates whose bounding boxes
/// overlap. Only checks each pair once (j > i). Early-exits on first intersection.
///
/// The 2D envelope prunes on both x *and* y, which is strictly more selective
/// than a 1D interval tree — especially for radial geometries like star-bursts
/// where edges in different quadrants have disjoint y-ranges.
pub(crate) fn has_self_intersections(coords: &[Coord<f64>], eps: f64) -> bool {
    let n = coords.len();
    let n_edges = n.saturating_sub(1);

    let edges: Vec<EdgeEnvelope> = (0..n_edges)
        .map(|i| EdgeEnvelope {
            index: i as u32,
            envelope: edge_envelope(coords, i),
        })
        .collect();

    let tree = RTree::bulk_load(edges);

    for i in 0..n_edges {
        let query_env = edge_envelope(coords, i);
        let result = tree.locate_in_envelope_intersecting_int(query_env, |candidate| {
            let j = candidate.index as usize;
            if j <= i {
                return std::ops::ControlFlow::Continue(());
            }
            if i.abs_diff(j) <= 1 || (i == 0 && j == n_edges - 1) {
                return std::ops::ControlFlow::Continue(());
            }
            if coords[i] == coords[j]
                && orient2d_fast(coords[i], coords[i + 1], coords[j + 1]) != 0.0
            {
                return std::ops::ControlFlow::Continue(());
            }
            if coords[i] == coords[j + 1]
                && orient2d_fast(coords[i], coords[i + 1], coords[j]) != 0.0
            {
                return std::ops::ControlFlow::Continue(());
            }
            if coords[i + 1] == coords[j]
                && orient2d_fast(coords[i + 1], coords[i], coords[j + 1]) != 0.0
            {
                return std::ops::ControlFlow::Continue(());
            }
            if coords[i + 1] == coords[j + 1]
                && orient2d_fast(coords[i + 1], coords[i], coords[j]) != 0.0
            {
                return std::ops::ControlFlow::Continue(());
            }

            if super::fix_ring::check_edge_pair(coords, i, j, eps) {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        });

        if result.is_break() {
            return true;
        }
    }

    false
}

/// Find the FIRST proper edge intersection using an R-tree.
/// Returns `(edge_i, edge_j, intersection_point)` with `i < j`.
///
/// This is the early-exit variant of `has_self_intersections` that also
/// returns the intersection geometry — used by the ATR fast-path ring
/// splitter.  O(n log n) average, exits after the first crossing found.
pub(crate) fn find_first_intersection(
    coords: &[Coord<f64>],
    eps: f64,
) -> Option<(usize, usize, Coord<f64>)> {
    let n = coords.len();
    let n_edges = n.saturating_sub(1);

    let edges: Vec<EdgeEnvelope> = (0..n_edges)
        .map(|i| EdgeEnvelope {
            index: i as u32,
            envelope: edge_envelope(coords, i),
        })
        .collect();

    let tree = RTree::bulk_load(edges);

    for i in 0..n_edges {
        let query_env = edge_envelope(coords, i);
        let result: std::ops::ControlFlow<Option<(usize, usize, Coord<f64>)>> = tree
            .locate_in_envelope_intersecting_int(query_env, |candidate| {
                let j = candidate.index as usize;
                if j <= i {
                    return std::ops::ControlFlow::Continue(());
                }
                if i.abs_diff(j) <= 1 || (i == 0 && j == n_edges - 1) {
                    return std::ops::ControlFlow::Continue(());
                }
                if coords[i] == coords[j]
                    && orient2d_fast(coords[i], coords[i + 1], coords[j + 1]) != 0.0
                {
                    return std::ops::ControlFlow::Continue(());
                }
                if coords[i] == coords[j + 1]
                    && orient2d_fast(coords[i], coords[i + 1], coords[j]) != 0.0
                {
                    return std::ops::ControlFlow::Continue(());
                }
                if coords[i + 1] == coords[j]
                    && orient2d_fast(coords[i + 1], coords[i], coords[j + 1]) != 0.0
                {
                    return std::ops::ControlFlow::Continue(());
                }
                if coords[i + 1] == coords[j + 1]
                    && orient2d_fast(coords[i + 1], coords[i], coords[j]) != 0.0
                {
                    return std::ops::ControlFlow::Continue(());
                }
                match super::fix_ring::edge_intersection(coords, i, j, eps) {
                    Some(pair) => std::ops::ControlFlow::Break(Some(pair)),
                    None => std::ops::ControlFlow::Continue(()),
                }
            });

        if let std::ops::ControlFlow::Break(Some(pair)) = result {
            return Some(pair);
        }
    }

    None
}

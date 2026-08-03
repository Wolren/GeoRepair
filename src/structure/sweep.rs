use geo::Coord;
use rstar::{RTree, RTreeObject, AABB};

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

/// PROPER self-crossing detection over a whole polygon (exterior + holes)
/// using an R-tree over edge bounding boxes. Interior-interior crossings
/// only - shared endpoints (hole touching shell at a vertex, which GEOS
/// makeValid legitimately emits) do NOT count.
///
/// This is the sweep variant of `has_proper_self_crossing`: O(n log n)
/// average instead of the brute-force O(n²) pair loop, which is fatal on
/// large rings (measured: 59k verts → 9.1s brute force, 181k verts → 143s).
pub(crate) fn has_proper_self_crossing_sweep(
    coords: &[Coord<f64>],
    ring_offsets: &[usize],
    _eps: f64,
) -> bool {
    let n_edges = coords.len().saturating_sub(1);
    if n_edges < 4 {
        return false;
    }
    let edges: Vec<EdgeEnvelope> = (0..n_edges)
        .filter(|&i| !is_phantom_segment(i, ring_offsets))
        .map(|i| EdgeEnvelope {
            index: i as u32,
            envelope: edge_envelope(coords, i),
        })
        .collect();

    let tree = RTree::bulk_load(edges);

    for i in 0..n_edges {
        // Skip phantom segments (closure-vertex boundary lines that don't
        // exist in the geometry) when QUERYING too.
        if is_phantom_segment(i, ring_offsets) {
            continue;
        }
        let query_env = edge_envelope(coords, i);
        let result = tree.locate_in_envelope_intersecting_int(query_env, |candidate| {
            let j = candidate.index as usize;
            if j <= i {
                return std::ops::ControlFlow::Continue(());
            }
            // Same-ring adjacent segments share an endpoint by construction.
            if segments_adjacent_in_ring(i, j, ring_offsets) {
                return std::ops::ControlFlow::Continue(());
            }
            if super::fix_ring::segments_properly_cross_seg(
                coords[i],
                coords[i + 1],
                coords[j],
                coords[j + 1],
            ) {
                if std::env::var("DIAG_SWEEP_CROSS").is_ok() {
                    eprintln!(
                        "SWEEP CROSS i={i} j={j} a=({:.6},{:.6})-({:.6},{:.6}) b=({:.6},{:.6})-({:.6},{:.6})",
                        coords[i].x, coords[i].y, coords[i + 1].x, coords[i + 1].y,
                        coords[j].x, coords[j].y, coords[j + 1].x, coords[j + 1].y
                    );
                }
                std::ops::ControlFlow::Break(())
            } else if ring_of_segment(i, ring_offsets) == ring_of_segment(j, ring_offsets)
                && crate::validation::edges_vertex_on_edge(
                    coords[i],
                    coords[i + 1],
                    coords[j],
                    coords[j + 1],
                )
            {
                // Same-ring vertex-on-edge self-touch (T-junction): GEOS
                // rejects a ring vertex on a non-adjacent edge (Test 22).
                // Cross-ring pairs stay untouched (hole vertex on shell
                // edge is a VALID OGC touch).
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

/// True if segment index `s` is a PHANTOM - the last index before a ring
/// boundary, which spans (closure vertex of ring r) → (first vertex of ring
/// r+1). That line does not exist in the geometry (the flat array just
/// concatenates rings) and must never be tested. Note: the closure vertex of
/// the LAST ring is covered by the ring's final real segment (end-2 → end-1),
/// so no phantom exists after the last ring.
fn is_phantom_segment(s: usize, ring_offsets: &[usize]) -> bool {
    ring_offsets.iter().skip(1).any(|&off| s + 1 == off)
}

/// Ring index (0 = exterior) of segment `s` in the flat coord slice.
fn ring_of_segment(s: usize, ring_offsets: &[usize]) -> usize {
    let mut r = 0;
    for (k, &off) in ring_offsets.iter().enumerate() {
        if off > s {
            break;
        }
        r = k;
    }
    r
}

/// True if segment indices `i` and `j` are adjacent within the SAME ring
/// (consecutive segments, or first/last of a closed ring). Ring offsets are
/// the start index of each ring in the flat coord slice (offset 0 = exterior).
fn segments_adjacent_in_ring(i: usize, j: usize, ring_offsets: &[usize]) -> bool {
    if is_phantom_segment(j, ring_offsets) {
        return true;
    }
    // Find the ring containing i (offsets are sorted; last ring runs to end).
    let mut r = 0;
    for (k, &off) in ring_offsets.iter().enumerate() {
        if off > i {
            break;
        }
        r = k;
    }
    let start = ring_offsets[r];
    let end = ring_offsets.get(r + 1).copied().unwrap_or(usize::MAX);
    // Segment k spans [k, k+1); the ring's real segments are [start, end-2]
    // (end-1 is the phantom spanning into the next ring).
    let seg_end = end.saturating_sub(1);
    let in_ring = |s: usize| s >= start && s < seg_end;
    if !in_ring(j) {
        return false;
    }
    if j == i + 1 {
        return true;
    }
    // First and last real segment of a closed ring share the closure vertex.
    j == start && i == seg_end - 1
}


/// Self-intersection detection using an R-tree over edge bounding boxes.
///
/// Builds an `rstar::RTree` keyed by each edge's 2D bounding envelope, then
/// queries each edge against the tree to find candidates whose bounding boxes
/// overlap. Only checks each pair once (j > i). Early-exits on first intersection.
///
/// The 2D envelope prunes on both x *and* y, which is strictly more selective
/// than a 1D interval tree - especially for radial geometries like star-bursts
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

    // Per-edge queries are independent — parallelize with find_any, which
    // short-circuits the whole search on the first crossing while still
    // splitting the 260k-edge giant shells across workers. Same pair
    // semantics as the serial loop below (identical closure body).
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        use rayon::prelude::*;
        (0..n_edges).into_par_iter().find_any(|&i| {
            let query_env = edge_envelope(coords, i);
            let mut found = false;
            let _ = tree.locate_in_envelope_intersecting_int(query_env, |candidate| {
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
                    found = true;
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::Continue(())
                }
            });
            found
        })
        .is_some()
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
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
}

/// Find the FIRST proper edge intersection using an R-tree.
/// Returns `(edge_i, edge_j, intersection_point)` with `i < j`.
///
/// This is the early-exit variant of `has_self_intersections` that also
/// returns the intersection geometry - used by the ATR fast-path ring
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

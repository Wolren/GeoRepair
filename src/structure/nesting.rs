//! hole nesting resolution, ring helpers, collapse handling
//!
//! Extracted from structure/mod.rs on 2026-08-07 (file-size governance).
//! Content is verbatim - no behavior changes; items are re-exported by
//! structure/mod.rs so `crate::structure::X` paths keep resolving.

use crate::core::MakeValidConfig;
use crate::util;
use alloc::vec::Vec;
use geo::{Coord, Geometry, LineString, Point, Polygon, Winding};
use rstar::{AABB, RTree, RTreeObject};

/// True if the polygon's linework has a PROPER self-crossing (interior-interior
/// intersection). Shared endpoints (hole touching shell at a vertex — GEOS
/// makeValid emits them) are legal and do NOT count. Used as a post-fix
/// filter: only genuine floating-point self-crossings are discarded.
/// Uses the R-tree sweep for O(n log n) instead of the brute-force O(n²)
/// pair loop — the quadratic version was fatal on large rings (59k verts →
/// 9.1s, 181k verts → 143s measured on the real-world dataset).
pub fn bbox_test(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    crate::simd::aabb_minmax_simd(coords)
}
pub fn eps_test(coords: &[Coord<f64>]) -> f64 {
    let b = crate::simd::aabb_minmax_simd(coords);
    let scale = (b.1 - b.0).abs().max((b.3 - b.2).abs()).max(1.0);
    crate::core::EPS * scale
}
pub fn has_proper_self_crossing(p: &geo::Polygon<f64>) -> bool {
    // Flatten exterior + holes into one coord slice, remembering ring starts.
    let mut coords: Vec<Coord<f64>> = Vec::with_capacity(
        p.exterior().0.len() + p.interiors().iter().map(|h| h.0.len()).sum::<usize>(),
    );
    let mut ring_offsets: Vec<usize> = Vec::with_capacity(p.interiors().len() + 1);
    ring_offsets.push(0);
    coords.extend_from_slice(&p.exterior().0);
    for h in p.interiors() {
        ring_offsets.push(coords.len());
        coords.extend_from_slice(&h.0);
    }
    if coords.len() < 4 {
        return false;
    }
    let bbox = crate::simd::aabb_minmax_simd(&coords);
    let scale = (bbox.1 - bbox.0)
        .abs()
        .max((bbox.3 - bbox.2).abs())
        .max(1.0);
    let eps = crate::core::EPS * scale;
    crate::structure::sweep::has_proper_self_crossing_sweep(&coords, &ring_offsets, eps)
}

/// Winding-number point-in-ring test (exclusive of boundary).
/// Delegates to SIMD-accelerated implementation.
fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    crate::simd::point_in_ring_exclusive(pt, ring)
}

/// True if the hole ring has at least one vertex STRICTLY OUTSIDE the shell
/// ring (neither inside nor on the boundary) - i.e. the hole crosses the
/// shell boundary. Boundary-touching holes (all vertices exactly on the
/// shell, e.g. CGAL square_hole_rhombus) return false. Used to route
/// crossing holes to the arrange fallback (see fix_polygon).
pub(crate) fn hole_vertex_strictly_outside(
    hole: &LineString<f64>,
    shell: &LineString<f64>,
) -> bool {
    let ring = shell.0.as_slice();
    if ring.len() < 4 {
        return false;
    }
    for &pt in &hole.0 {
        if point_in_ring_exclusive(pt, ring) {
            continue;
        }
        // On the boundary? Exact-vertex touch: distance to the nearest shell
        // segment within the validation tolerance (1e-12 * L^2 relative).
        let mut on_boundary = false;
        for w in ring.windows(2) {
            if w[0] == w[1] {
                continue;
            }
            let dx = w[1].x - w[0].x;
            let dy = w[1].y - w[0].y;
            let len2 = dx * dx + dy * dy;
            if len2 == 0.0 {
                continue;
            }
            let t = ((pt.x - w[0].x) * dx + (pt.y - w[0].y) * dy) / len2;
            let t = t.clamp(0.0, 1.0);
            let px = w[0].x + t * dx;
            let py = w[0].y + t * dy;
            let d2 = (pt.x - px) * (pt.x - px) + (pt.y - py) * (pt.y - py);
            if d2 <= 1e-12 * len2 {
                on_boundary = true;
                break;
            }
        }
        if !on_boundary {
            return true;
        }
    }
    false
}

/// Compute bounding box of a coordinate ring as (min_x, max_x, min_y, max_y).
pub(crate) fn ring_bbox(coords: &[Coord<f64>]) -> (f64, f64, f64, f64) {
    crate::simd::aabb_minmax_simd(coords)
}

/// True if EVERY vertex of `inner` is strictly inside ring `outer`.
/// Used for hole-role swap: an outer hole that fully contains the shell
/// becomes the shell itself (GEOS even-odd semantics).
pub(crate) fn all_vertices_inside_ring(inner: &[Coord<f64>], outer: &[Coord<f64>]) -> bool {
    if inner.len() < 4 || outer.len() < 4 {
        return false;
    }
    inner.iter().all(|pt| point_in_ring_exclusive(*pt, outer))
}

/// Check if two bounding boxes overlap.
#[inline]
pub(crate) fn bboxes_overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 <= b.1 && a.1 >= b.0 && a.2 <= b.3 && a.3 >= b.2
}

/// Resolve hole-hole nesting among inner holes of a shell.
///
/// Returns:
/// - `to_subtract`: holes at containment depth 1 (directly inside the shell).
///   These are subtracted from the shell via boolean difference.
/// - `islands`: holes at depth 2+ become separate polygons, with their own
///   sub-holes (depth 3) as interior rings. Depth alternates: even depths are
///   separate polygons (islands/positive space), odd depths are holes (negative space).
pub(crate) fn resolve_nesting(
    holes: &[LineString<f64>],
) -> (Vec<LineString<f64>>, Vec<Polygon<f64>>) {
    if holes.len() <= 1 {
        return (holes.to_vec(), Vec::new());
    }

    // Build parent relationship: hole[j] is inside hole[i] → parent_of[j] = Some(i)
    let n = holes.len();

    // Precompute bbox + area for each hole, then build R-tree for O(log n) lookup
    #[derive(Clone, Copy)]
    struct HoleEnv {
        idx: usize,
        env: AABB<[f64; 2]>,
        area: f64,
    }
    impl RTreeObject for HoleEnv {
        type Envelope = AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }
    let envs: Vec<HoleEnv> = holes
        .iter()
        .enumerate()
        .filter_map(|(i, h)| {
            let first = h.0.first()?;
            let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
            for c in &h.0 {
                min_x = min_x.min(c.x);
                max_x = max_x.max(c.x);
                min_y = min_y.min(c.y);
                max_y = max_y.max(c.y);
            }
            Some(HoleEnv {
                idx: i,
                env: AABB::from_corners([min_x, min_y], [max_x, max_y]),
                area: util::shoelace_sum(&h.0).abs() / 2.0,
            })
        })
        .collect();
    let tree = RTree::bulk_load(envs);

    let parent_of: Vec<Option<usize>> = {
        let find_parent = |j: usize| -> Option<usize> {
            let pt = *holes[j].0.first()?;
            let query = AABB::from_corners([pt.x, pt.y], [pt.x, pt.y]);
            let mut best: Option<usize> = None;
            let mut best_area = f64::MAX;
            let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
                if c.idx == j {
                    return ::core::ops::ControlFlow::<(), ()>::Continue(());
                }
                if point_in_ring_exclusive(pt, &holes[c.idx].0) && c.area < best_area {
                    best_area = c.area;
                    best = Some(c.idx);
                }
                ::core::ops::ControlFlow::<(), ()>::Continue(())
            });
            best
        };
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        if n >= 8 {
            use rayon::prelude::*;
            (0..n).into_par_iter().map(find_parent).collect()
        } else {
            (0..n).map(find_parent).collect()
        }
        #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
        {
            (0..n).map(find_parent).collect()
        }
    };

    // Compute containment depth for each hole via BFS topological sort
    let mut depth = vec![0usize; n];
    let mut children = vec![Vec::new(); n];
    let mut queue: Vec<usize> = Vec::with_capacity(n);
    for (i, p) in parent_of.iter().enumerate() {
        if let Some(p) = p {
            children[*p].push(i);
        } else {
            depth[i] = 1;
            queue.push(i);
        }
    }
    let mut head = 0;
    while head < queue.len() {
        let p = queue[head];
        head += 1;
        for &child in &children[p] {
            depth[child] = depth[p] + 1;
            queue.push(child);
        }
    }

    // Group holes by depth parity:
    // even depth (2, 4, ...): separate polygons (islands)
    // odd depth (1, 3, ...): subtract-from-parent (holes/voids)
    let mut subtract = Vec::new();
    let mut island_indices = Vec::new();
    for (i, &d) in depth.iter().enumerate() {
        if d == 0 {
            // Unreachable (shouldn't happen), treat as top-level hole
            subtract.push(i);
        } else if d % 2 == 1 {
            subtract.push(i);
        } else {
            island_indices.push(i);
        }
    }

    // For depth-2+ holes (islands), assign depth-3+ children as interior rings
    // Build island polygons with proper sub-hole nesting
    let mut islands: Vec<Polygon<f64>> = Vec::new();
    for &ii in &island_indices {
        let children: Vec<LineString<f64>> = (0..n)
            .filter(|&j| parent_of[j] == Some(ii) && depth[j] > depth[ii] && depth[j] % 2 == 1)
            .map(|j| holes[j].clone())
            .collect();
        islands.push(Polygon::new(holes[ii].clone(), children));
    }

    (
        subtract.into_iter().map(|i| holes[i].clone()).collect(),
        islands,
    )
}

pub(crate) fn ensure_ccw(mut ring: LineString<f64>) -> LineString<f64> {
    #[cfg(feature = "simd")]
    let ccw = crate::simd::is_ring_ccw_simd(&ring.0);
    #[cfg(not(feature = "simd"))]
    let ccw = ring.winding_order() == Some(geo::winding_order::WindingOrder::CounterClockwise);
    if !ccw {
        ring.make_ccw_winding();
    }
    ring
}

pub(crate) fn ensure_cw(mut ring: LineString<f64>) -> LineString<f64> {
    if ring.winding_order() != Some(geo::winding_order::WindingOrder::Clockwise) {
        ring.make_cw_winding();
    }
    ring
}

/// When keep_collapsed is true and the polygon shell collapsed during repair,
/// return a Point or LineString instead of empty.
pub(crate) fn handle_collapse_result(
    exterior: &LineString<f64>,
    _config: &MakeValidConfig,
) -> Option<Geometry<f64>> {
    let coords: Vec<Coord<f64>> = exterior
        .0
        .iter()
        .copied()
        .filter(|c| c.x.is_finite() && c.y.is_finite())
        .collect();
    match coords.len() {
        0 => None,
        1 => Some(Geometry::Point(Point(coords[0]))),
        _ => {
            let deduped: Vec<Coord<f64>> = {
                let mut v = Vec::with_capacity(coords.len());
                for c in coords {
                    if v.last() != Some(&c) {
                        v.push(c);
                    }
                }
                v
            };
            if deduped.len() == 1 {
                Some(Geometry::Point(Point(deduped[0])))
            } else {
                Some(Geometry::LineString(LineString::new(deduped)))
            }
        }
    }
}

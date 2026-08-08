//! Ring-touch graph cycle detection for disconnected-interior checks
//! (split from validation/holes.rs 2026-08-07; verbatim).

use super::*;
use alloc::vec::Vec;

/// Ring-touch graph cycle detection (GEOS PolygonRing::findHoleCycleLocation
/// and scanForHoleCycle port). Touches are exact shared vertices plus
/// vertex-on-edge contacts within eps. Returns true when the touch graph
/// contains a cycle through pairwise-DISTINCT coordinates (disconnected
/// interior); touches at a single coordinate never close a cycle (GEOS
/// isValid=true for multiple holes meeting at one point).
///
/// Per-ring structures (sorted unique vertex list, edge tree for large
/// rings) are built ONCE outside the pair loop - never re-sorted or
/// re-indexed per pair (the naive version cost 170x on the real-world
/// giants, measured 2026-08-06).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn detect_hole_cycle(
    rings: &[&[Coord<f64>]],
    bboxes: &[[f64; 4]],
    eps: f64,
    #[cfg(feature = "rstar")] shell_tree: Option<&Arc<rstar::RTree<EdgeIdx>>>,
) -> bool {
    let n = rings.len();
    // Per-ring structures built ONCE (never per pair): a sorted unique
    // vertex list (x-range probing only - shared vertices come from the
    // by_coord pass below; the shell's shared vertices are additionally
    // covered by ring_touch_points in check_holes_valid) and an edge tree
    // for large rings. The shell's tree is REUSED from check_holes_valid
    // (rings[0] is the shell) - building a second rstar bulk_load of a
    // 600k-edge shell cost ~100ms per giant (measured 2026-08-06). The
    // vertex lists are radix-sorted by x (std sort cost ~50ms on the
    // giant shells; measured 2026-08-06).
    let mut sorted: Vec<Vec<Coord<f64>>> = Vec::with_capacity(n);
    for r in rings {
        let mut v = r.to_vec();
        let mut keys: Vec<u64> = v.iter().map(|c| sortable_u64(c.x)).collect();
        let mut order: Vec<u32> = (0..v.len() as u32).collect();
        radix_sort_keys_tls(&mut keys, &mut order);
        let mut perm: Vec<Coord<f64>> = Vec::with_capacity(v.len());
        for &i in &order {
            perm.push(v[i as usize]);
        }
        v = perm;
        sorted.push(v);
    }
    #[cfg(feature = "rstar")]
    let edge_trees: Vec<Option<Arc<rstar::RTree<EdgeIdx>>>> = rings
        .iter()
        .enumerate()
        .map(|(i, r)| {
            if i == 0 {
                shell_tree.cloned()
            } else if r.len() - 1 > 64 {
                Some(Arc::new(build_ring_edge_tree(r)))
            } else {
                None
            }
        })
        .collect();
    let mut touches: Vec<Vec<(usize, Coord<f64>)>> = vec![Vec::new(); n];
    // Exact shared vertices among the HOLES: one global pass (vertex ->
    // containing rings), O(total vertices); -0.0 normalizes to 0.0 so both
    // spellings match. The shell (ring 0) is skipped: its shared vertices
    // are hole vertices lying on shell edges, already collected exactly by
    // ring_touch_points in check_holes_valid (the shell's vertex-on-edge
    // probes below are kept for shell vertices on hole edges). Skipping the
    // shell removes ~600k hash inserts per giant (measured 2026-08-06).
    let mut by_coord: rustc_hash::FxHashMap<(u64, u64), Vec<usize>> =
        rustc_hash::FxHashMap::with_capacity_and_hasher(64, Default::default());
    for (ri, ring) in rings.iter().enumerate().skip(1) {
        for &v in *ring {
            let k = (
                if v.x == 0.0 { 0u64 } else { v.x.to_bits() },
                if v.y == 0.0 { 0u64 } else { v.y.to_bits() },
            );
            by_coord.entry(k).or_default().push(ri);
        }
    }
    for (&k, rs) in by_coord.iter() {
        if rs.len() < 2 {
            continue;
        }
        let c = Coord {
            x: f64::from_bits(k.0),
            y: f64::from_bits(k.1),
        };
        for (i, &a) in rs.iter().enumerate() {
            for &b in rs.iter().skip(i + 1) {
                if a != b {
                    touches[a].push((b, c));
                    touches[b].push((a, c));
                }
            }
        }
    }
    // Vertex-on-edge contacts, both directions (duplicates deduped below).
    // Each probe iterates only the source vertices inside the target's x
    // range (binary search over the precomputed sorted list), so giant
    // shells probe each hole in O(log V) + near-hole vertices.
    for (a, b) in overlap_pairs(bboxes, 0) {
        let ra = rings[a];
        let rb = rings[b];
        let n_a = ra.len() - 1;
        let n_b = rb.len() - 1;
        if n_a > 64 || n_b > 64 {
            #[cfg(feature = "rstar")]
            {
                collect_on_edge_tree(a, b, &sorted[a], rb, bboxes, eps, &edge_trees, &mut touches);
                collect_on_edge_tree(b, a, &sorted[b], ra, bboxes, eps, &edge_trees, &mut touches);
            }
            #[cfg(not(feature = "rstar"))]
            {
                collect_on_edge_brute(a, b, &sorted[a], rb, bboxes, eps, &mut touches);
                collect_on_edge_brute(b, a, &sorted[b], ra, bboxes, eps, &mut touches);
            }
        } else {
            collect_on_edge_brute(a, b, &sorted[a], rb, bboxes, eps, &mut touches);
            collect_on_edge_brute(b, a, &sorted[b], ra, bboxes, eps, &mut touches);
        }
    }
    for t in touches.iter_mut() {
        t.sort_by(|x, y| {
            x.1.x
                .total_cmp(&y.1.x)
                .then(x.1.y.total_cmp(&y.1.y))
                .then(x.0.cmp(&y.0))
        });
        t.dedup_by(|x, y| x.0 == y.0 && x.1.x == y.1.x && x.1.y == y.1.y);
    }
    // GEOS findHoleCycleLocation: per-root DFS over the touch graph; a ring
    // reached through two different touch paths closes a cycle.
    let mut touch_set_root: Vec<Option<usize>> = vec![None; n];
    let mut stack: Vec<(usize, Coord<f64>)> = Vec::new();
    for root in 0..n {
        if touch_set_root[root].is_some() {
            continue;
        }
        touch_set_root[root] = Some(root);
        if touches[root].is_empty() {
            continue;
        }
        // Init: push ALL of root's touches (GEOS init; nothing is marked
        // yet, so double-touches of the same ring at different coordinates
        // both enter the stack and close a cycle on the second scan).
        for &(other, coord) in &touches[root] {
            stack.push((other, coord));
        }
        while let Some((ring, entry)) = stack.pop() {
            for &(other, coord) in &touches[ring] {
                if coord.x == entry.x && coord.y == entry.y {
                    continue;
                }
                if touch_set_root[other] == Some(root) {
                    return true;
                }
                if touch_set_root[other].is_none() {
                    touch_set_root[other] = Some(root);
                    stack.push((other, coord));
                }
            }
        }
    }
    false
}

/// Brute-force vertex-on-edge touch collection (small rings). Iterates only
/// the source vertices inside the target's x range (binary search over the
/// precomputed sorted list) and the target's bbox - giant shells probe each
/// hole in O(log V) + near-hole vertices.
pub(super) fn collect_on_edge_brute(
    src: usize,
    tgt: usize,
    src_sorted: &[Coord<f64>],
    tgt_ring: &[Coord<f64>],
    bboxes: &[[f64; 4]],
    eps: f64,
    touches: &mut [Vec<(usize, Coord<f64>)>],
) {
    let tb = bboxes[tgt];
    let n_t = tgt_ring.len().saturating_sub(1);
    if n_t < 1 {
        return;
    }
    let lo = src_sorted.partition_point(|v| v.x < tb[0] - eps);
    let hi = src_sorted.partition_point(|v| v.x <= tb[2] + eps);
    for &v in &src_sorted[lo..hi] {
        if v.y < tb[1] - eps || v.y > tb[3] + eps {
            continue;
        }
        let mut hit = false;
        for i in 0..n_t {
            // EXACT on-edge test: a touch is a zero-distance contact (GEOS
            // parity, see point_on_segment_exact). The tolerance version
            // fabricated touches from near-miss vertices in real cadastral
            // giants (GEOS isValid=true, measured 2026-08-06).
            if point_on_segment_exact(v, tgt_ring[i], tgt_ring[(i + 1) % n_t]) {
                hit = true;
                break;
            }
        }
        if hit {
            touches[src].push((tgt, v));
            touches[tgt].push((src, v));
        }
    }
}

/// Edge-tree vertex-on-edge touch collection (large target ring). The
/// source side iterates only x-range-gated vertices (binary search over the
/// precomputed sorted list); small target rings fall back to brute force
/// over their edges for the few in-range vertices.
#[cfg(feature = "rstar")]
// All parameters are primitive/borrowed predicates of the cycle probe;
// grouping them into a struct would churn six call sites for no behavior
// gain. Internal helper, not part of the public surface.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_on_edge_tree(
    src: usize,
    tgt: usize,
    src_sorted: &[Coord<f64>],
    tgt_ring: &[Coord<f64>],
    bboxes: &[[f64; 4]],
    eps: f64,
    edge_trees: &[Option<Arc<rstar::RTree<EdgeIdx>>>],
    touches: &mut [Vec<(usize, Coord<f64>)>],
) {
    let tb = bboxes[tgt];
    let n_t = tgt_ring.len().saturating_sub(1);
    if n_t < 1 {
        return;
    }
    let lo = src_sorted.partition_point(|v| v.x < tb[0] - eps);
    let hi = src_sorted.partition_point(|v| v.x <= tb[2] + eps);
    let tree = match &edge_trees[tgt] {
        Some(t) => t,
        None => {
            // Small target ring: brute-force its edges for the few source
            // vertices in its x range and bbox. EXACT on-edge test - see
            // collect_on_edge_brute for the parity note (eps fabricated
            // touches on real cadastral giants, GEOS isValid=true).
            for &v in &src_sorted[lo..hi] {
                if v.y < tb[1] - eps || v.y > tb[3] + eps {
                    continue;
                }
                for i in 0..n_t {
                    if point_on_segment_exact(v, tgt_ring[i], tgt_ring[(i + 1) % n_t]) {
                        touches[src].push((tgt, v));
                        touches[tgt].push((src, v));
                        break;
                    }
                }
            }
            return;
        }
    };
    for &v in &src_sorted[lo..hi] {
        if v.y < tb[1] - eps || v.y > tb[3] + eps {
            continue;
        }
        let q = rstar::AABB::from_corners([v.x - eps, v.y - eps], [v.x + eps, v.y + eps]);
        let hit = tree
            .locate_in_envelope_intersecting_int(q, |c| {
                if point_on_segment_exact(v, tgt_ring[c.idx], tgt_ring[(c.idx + 1) % n_t]) {
                    core::ops::ControlFlow::Break(())
                } else {
                    core::ops::ControlFlow::<(), ()>::Continue(())
                }
            })
            .is_break();
        if hit {
            touches[src].push((tgt, v));
            touches[tgt].push((src, v));
        }
    }
}

/// First ring vertex after `p0` that is not exactly equal to it.
pub(super) fn first_non_equal(ring: &[Coord<f64>], p0: Coord<f64>) -> Option<Coord<f64>> {
    ring.iter()
        .skip(1)
        .find(|&&c| c.x != p0.x || c.y != p0.y)
        .copied()
}

/// GEOS PolygonTopologyAnalyzer::isRingNested port: is `test` nested inside
/// `target`? A start point strictly inside => yes; outside => no; on the
/// boundary => the incident-segment topology decides.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn is_ring_nested(test: &[Coord<f64>], target: &[Coord<f64>], eps: f64) -> bool {
    let Some(&p0) = test.first() else {
        return false;
    };
    if target.len() < 4 {
        return false;
    }
    if point_on_ring(p0, target, eps) {
        let Some(p1) = first_non_equal(test, p0) else {
            return false;
        };
        return is_incident_segment_in_ring(p0, p1, target, eps);
    }
    point_in_ring_exclusive(p0, target)
}

/// GEOS PolygonTopologyAnalyzer::isIncidentSegmentInRing port: is the
/// segment p0->p1 (p0 on the ring boundary) inside the ring at the node?
pub(super) fn is_incident_segment_in_ring(
    p0: Coord<f64>,
    p1: Coord<f64>,
    target: &[Coord<f64>],
    eps: f64,
) -> bool {
    let n = target.len() - 1;
    if n < 2 {
        return false;
    }
    // Segment containing p0 (GEOS intersectingSegIndex: p0 == segment end
    // selects the NEXT segment start).
    let mut idx = 0usize;
    for i in 0..n {
        if point_on_segment(p0, target[i], target[(i + 1) % n], eps) {
            idx = if p0.x == target[i + 1].x && p0.y == target[i + 1].y {
                i + 1
            } else {
                i
            };
            break;
        }
    }
    if idx >= n {
        idx = 0;
    }
    // Prev/next ring vertices, walking away from coordinates equal to p0.
    let mut i_prev = idx;
    for _ in 0..n {
        if target[i_prev].x != p0.x || target[i_prev].y != p0.y {
            break;
        }
        i_prev = (i_prev + n - 1) % n;
    }
    let mut i_next = idx;
    for _ in 0..n {
        if target[i_next].x != p0.x || target[i_next].y != p0.y {
            break;
        }
        i_next = (i_next + 1) % n;
    }
    let (mut a0, mut a1) = (target[i_prev], target[i_next]);
    // GEOS: interior on the right for CW rings; CCW rings swap prev/next so
    // the corner is traversed with the interior wedge on the left.
    if crate::util::robust_is_ccw(target) {
        core::mem::swap(&mut a0, &mut a1);
    }
    is_interior_segment(p0, a0, a1, p1)
}

/// GEOS PolygonNodeTopology::isInteriorSegment port.
pub(super) fn is_interior_segment(
    node: Coord<f64>,
    a0: Coord<f64>,
    a1: Coord<f64>,
    b: Coord<f64>,
) -> bool {
    let (mut a_lo, mut a_hi) = (a0, a1);
    let mut is_interior_between = true;
    if is_angle_greater(node, a_lo, a_hi) {
        core::mem::swap(&mut a_lo, &mut a_hi);
        is_interior_between = false;
    }
    let b_between = is_between(node, b, a_lo, a_hi);
    (b_between && is_interior_between) || (!b_between && !is_interior_between)
}

/// GEOS Quadrant + isAngleGreater port: p > q when p is CCW of q as seen
/// from the origin.
pub(super) fn is_angle_greater(origin: Coord<f64>, p: Coord<f64>, q: Coord<f64>) -> bool {
    let qp = quadrant(origin, p);
    let qq = quadrant(origin, q);
    if qp > qq {
        return true;
    }
    if qp < qq {
        return false;
    }
    // Same quadrant: p > q iff p is CCW of q (robust orient2d, > 0 = CCW).
    crate::orient::orient2d(origin, q, p) > 0.0
}

/// GEOS isBetween port: p lies in the CCW angle wedge e0 -> e1 from origin.
pub(super) fn is_between(
    origin: Coord<f64>,
    p: Coord<f64>,
    e0: Coord<f64>,
    e1: Coord<f64>,
) -> bool {
    if !is_angle_greater(origin, p, e0) {
        return false;
    }
    !is_angle_greater(origin, p, e1)
}

/// GEOS Quadrant numbering: NE=0, NW=1, SW=2, SE=3.
pub(super) fn quadrant(origin: Coord<f64>, p: Coord<f64>) -> u8 {
    let dx = p.x - origin.x;
    let dy = p.y - origin.y;
    debug_assert!(dx != 0.0 || dy != 0.0, "quadrant of zero vector");
    if dx >= 0.0 {
        if dy >= 0.0 { 0 } else { 3 }
    } else if dy >= 0.0 {
        1
    } else {
        2
    }
}

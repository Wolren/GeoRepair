
use super::*;
use alloc::vec::Vec;

/// Never re-sort or re-index per pair; the naive version cost 170x on giants
/// (2026-08-06).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn detect_hole_cycle(
    rings: &[&[Coord<f64>]],
    bboxes: &[[f64; 4]],
    eps: f64,
    #[cfg(feature = "rstar")] shell_tree: Option<&Arc<rstar::RTree<EdgeIdx>>>,
) -> bool {
    let n = rings.len();
    // The vertex lists are radix-sorted by x (std sort cost ~50ms on the giant shells.
    // Measured 2026-08-06).
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
    // Exact shared vertices among the holes: one global pass (vertex -> containing
    // rings), O(total vertices). -0.0 normalizes to 0.0 The shell (ring 0) is skipped
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
            // The tolerance version fabricated touches from near-miss vertices in real
            // cadastral giants (GEOS isValid=true, measured 2026-08-06).
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

#[cfg(feature = "rstar")]
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

/// GEOS Quadrant + isAngleGreater port: p > q when p is ccw of q as seen from the
/// origin.
pub(super) fn is_angle_greater(origin: Coord<f64>, p: Coord<f64>, q: Coord<f64>) -> bool {
    let qp = quadrant(origin, p);
    let qq = quadrant(origin, q);
    if qp > qq {
        return true;
    }
    if qp < qq {
        return false;
    }
    crate::orient::orient2d(origin, q, p) > 0.0
}

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

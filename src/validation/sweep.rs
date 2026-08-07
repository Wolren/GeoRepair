//! sweep-line machinery for ring self-intersection
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance:
//! core.rs was 2540 lines; the cap is 800). Content is verbatim - no
//! behavior changes; sibling modules resolve shared items through the
//! `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.



use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::validation::core::*;
use geo::{Coord};

/// Padded 2D bounding-box overlap for an edge pair (the R-tree envelope
/// filter equivalent: both boxes inflated by 1e-10 x max-dim, floored at
/// 1e-10). Conservative by construction: any pair the exact predicates
/// accept has overlapping boxes, so rejecting here never changes results.
#[inline]
pub(crate) fn padded_bbox_overlap(a1: Coord<f64>, a2: Coord<f64>, b1: Coord<f64>, b2: Coord<f64>) -> bool {
    let (lo_x, hi_x) = if a1.x < a2.x { (a1.x, a2.x) } else { (a2.x, a1.x) };
    let (lo_y, hi_y) = if a1.y < a2.y { (a1.y, a2.y) } else { (a2.y, a1.y) };
    let (lo_x2, hi_x2) = if b1.x < b2.x { (b1.x, b2.x) } else { (b2.x, b1.x) };
    let (lo_y2, hi_y2) = if b1.y < b2.y { (b1.y, b2.y) } else { (b2.y, b1.y) };
    let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
    let ext2 = (hi_x2 - lo_x2).abs().max((hi_y2 - lo_y2).abs()).max(1.0) * 1e-10;
    hi_x + ext >= lo_x2 - ext2
        && lo_x - ext <= hi_x2 + ext2
        && hi_y + ext >= lo_y2 - ext2
        && lo_y - ext <= hi_y2 + ext2
}

/// Order-preserving u64 encoding of an f64 (IEEE bit trick): positives map
/// above negatives, negatives reverse-magnitude. NaN handled upstream
/// (finite check in check_ring_validity).
#[inline]
pub(crate) fn sortable_u64(x: f64) -> u64 {
    let bits = x.to_bits();
    if bits >> 63 == 0 {
        bits | 0x8000_0000_0000_0000
    } else {
        !bits
    }
}

#[cfg(feature = "std")]
thread_local! {
    static SWEEP_SCRATCH: core::cell::RefCell<SweepScratch> =
        core::cell::RefCell::new(SweepScratch::default());
}

/// Run a closure against the radix-sort scratch buffers. With  the
/// buffers are thread-local (no realloc between sweeps); without it a
/// fresh default scratch is allocated per call (no_std has no TLS).
#[cfg(feature = "std")]
fn with_scratch<R>(f: impl FnOnce(&mut SweepScratch) -> R) -> R {
    SWEEP_SCRATCH.with(|s| f(&mut s.borrow_mut()))
}

#[cfg(not(feature = "std"))]
fn with_scratch<R>(f: impl FnOnce(&mut SweepScratch) -> R) -> R {
    f(&mut SweepScratch::default())
}

struct SweepScratch {
    keys: Vec<u64>,
    order: Vec<u32>,
    tmp_keys: Vec<u64>,
    tmp_order: Vec<u32>,
    counts: Box<[u32; 256]>,
    /// Padded bounds per ring edge: [lo_x, hi_x, lo_y, hi_y].
    spans: Vec<[f64; 4]>,
    active: Vec<u32>,
}

impl Default for SweepScratch {
    fn default() -> Self {
        SweepScratch {
            keys: Vec::new(),
            order: Vec::new(),
            tmp_keys: Vec::new(),
            tmp_order: Vec::new(),
            counts: Box::new([0u32; 256]),
            spans: Vec::new(),
            active: Vec::new(),
        }
    }
}

/// LSD radix sort of (keys, order) pairs, 8 bits x 8 passes. Stable; sorts
/// by ascending key. Buffers are passed in (the caller already holds the
/// TLS scratch borrow - re-borrowing would panic).
fn radix_sort_u64(
    keys: &mut Vec<u64>,
    order: &mut Vec<u32>,
    tmp_keys: &mut Vec<u64>,
    tmp_order: &mut Vec<u32>,
    counts: &mut [u32; 256],
) {
    let n = keys.len();
    tmp_keys.resize(n, 0);
    tmp_order.resize(n, 0);
    for shift in (0..64).step_by(8) {
        counts.fill(0);
        for &k in keys.iter() {
            counts[((k >> shift) & 0xff) as usize] += 1;
        }
        let mut acc = 0u32;
        for c in counts.iter_mut() {
            let t = *c;
            *c = acc;
            acc += t;
        }
        for i in 0..n {
            let k = keys[i];
            let b = ((k >> shift) & 0xff) as usize;
            let pos = counts[b] as usize;
            tmp_keys[pos] = k;
            tmp_order[pos] = order[i];
            counts[b] += 1;
        }
        core::mem::swap(tmp_keys, keys);
        core::mem::swap(tmp_order, order);
    }
}

/// Radix sort of (keys, order) using the TLS scratch buffers (no caller
/// borrow held). Used for the cycle-detection vertex lists.
pub(crate) fn radix_sort_keys_tls(keys: &mut Vec<u64>, order: &mut Vec<u32>) {
    with_scratch(|s| {
        let SweepScratch {
            tmp_keys,
            tmp_order,
            counts,
            ..
        } = s;
        radix_sort_u64(keys, order, tmp_keys, tmp_order, counts);
    });
}

/// Rings whose x-overlap active set exceeds this route to the spatial
/// tree / brute force instead of the linear-active sweep (which would be
/// O(n^2) on them). Measured worst real-world giant: 63.
const SWEEP_DENSE_ACTIVE_LIMIT: usize = 256;

/// Self-intersection sweep. Returns Some(true) on intersection, Some(false)
/// when clean, None when the ring's x-overlap density exceeds the routing
/// limit (caller falls back to the indexed / brute path).
///
/// `check_revisit`: the LINE path's vertex-revisit class (a non-adjacent
/// pair sharing an endpoint is non-simple). The equality compares run ONLY
/// on lean-escalated pairs - every revisit pair has an orient exactly zero,
/// so it always escalates, and fast-FP-strong dense pairs never pay the
/// compares (the star-comb's ~120k bbox-overlapping pairs measured clean;
/// 2026-08-07). The ring path passes false - shared vertices there are the
/// pinch class, classified by check_ring_validity.
pub(crate) fn sweep_ring_self_intersects(
    ring: &[Coord<f64>],
    eps: f64,
    check_revisit: bool,
) -> Option<bool> {
    let n = ring.len() - 1;
    let closed = ring[0] == ring[n];
    with_scratch(|s| {
        let SweepScratch {
            keys,
            order,
            spans,
            active,
            tmp_keys,
            tmp_order,
            counts,
        } = s;
        keys.clear();
        order.clear();
        spans.clear();
        keys.reserve(n);
        order.reserve(n);
        spans.reserve(n);
        let mut sorted = true;
        for i in 0..n {
            let a = ring[i];
            // min(n) identical to % n for closed rings (ring[n] == ring[0]),
            // correct for open chain slices (see check_edge_pair_intersection).
            let b = ring[(i + 1).min(n)];
            let (lo_x, hi_x) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
            let (lo_y, hi_y) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
            let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
            let k = sortable_u64(lo_x - ext);
            if i > 0 && k < keys[i - 1] {
                sorted = false;
            }
            keys.push(k);
            order.push(i as u32);
            spans.push([lo_x - ext, hi_x + ext, lo_y - ext, hi_y + ext]);
        }
        // Sortedness skip: x-ordered input (the common valid-polyline case)
        // needs no radix sort - the active-set logic runs in input order.
        // Measured (2026-08-07): the 8-pass radix was ~40% of the sweep's
        // fixed cost on a 500-vertex valid line.
        if sorted {
            order.clear();
            for i in 0..n {
                order.push(i as u32);
            }
        } else {
            radix_sort_u64(keys, order, tmp_keys, tmp_order, counts);
        }
        active.clear();
        for &ord in order.iter() {
            let r_i = ord as usize;
            let cur = spans[r_i];
            // Limit check BEFORE the retain: a pathological ring with a
            // growing active set would otherwise pay O(n x active) in
            // retains before ever reaching the check.
            if active.len() > SWEEP_DENSE_ACTIVE_LIMIT {
                return None;
            }
            active.retain(|&p| spans[p as usize][1] >= cur[0]);
            for &p in &*active {
                let t = spans[p as usize];
                // x-overlap is guaranteed by the retain condition; only the
                // y gate remains (matches the tree's 2D envelope filter).
                if t[3] < cur[2] || t[2] > cur[3] {
                    continue;
                }
                let r_j = p as usize;
                if r_i.abs_diff(r_j) <= 1 {
                    continue;
                }
                let a1 = ring[r_i];
                let a2 = ring[(r_i + 1).min(n)];
                let b1 = ring[r_j];
                let b2 = ring[(r_j + 1).min(n)];
                let mut ambiguous = false;
                if crate::validation::edges::lean_pair_intersects(
                    a1, a2, b1, b2, eps, &mut ambiguous,
                ) {
                    return Some(true);
                }
                if ambiguous
                    && check_revisit
                    && !(closed && (r_i == 0 && r_j == n - 1 || r_i == n - 1 && r_j == 0))
                    && (a1 == b1 || a1 == b2 || a2 == b1 || a2 == b2)
                {
                    return Some(true);
                }
            }
            active.push(r_i as u32);
        }
        Some(false)
    })
}

/// R-tree self-intersection check (the dense-ring fallback; kept exact -
/// same predicate and pair rules as the sweep).
#[cfg(feature = "rstar")]
pub(crate) fn ring_tree_self_intersects(ring: &[Coord<f64>], n: usize, eps: f64) -> bool {
    struct EdgeEnv {
        idx: u32,
        env: rstar::AABB<[f64; 2]>,
    }
    impl rstar::RTreeObject for EdgeEnv {
        type Envelope = rstar::AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }
    let mut edges = Vec::with_capacity(n);
    for i in 0..n {
        let (lo_x, hi_x) = if ring[i].x < ring[(i + 1) % n].x {
            (ring[i].x, ring[(i + 1) % n].x)
        } else {
            (ring[(i + 1) % n].x, ring[i].x)
        };
        let (lo_y, hi_y) = if ring[i].y < ring[(i + 1) % n].y {
            (ring[i].y, ring[(i + 1) % n].y)
        } else {
            (ring[(i + 1) % n].y, ring[i].y)
        };
        let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
        edges.push(EdgeEnv {
            idx: i as u32,
            env: rstar::AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext]),
        });
    }
    let tree = rstar::RTree::bulk_load(edges);
    for i in 0..n {
        let (lo_x, hi_x) = if ring[i].x < ring[(i + 1) % n].x {
            (ring[i].x, ring[(i + 1) % n].x)
        } else {
            (ring[(i + 1) % n].x, ring[i].x)
        };
        let (lo_y, hi_y) = if ring[i].y < ring[(i + 1) % n].y {
            (ring[i].y, ring[(i + 1) % n].y)
        } else {
            (ring[(i + 1) % n].y, ring[i].y)
        };
        let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
        let env = rstar::AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext]);
        let found = tree.locate_in_envelope_intersecting_int(env, |c| {
            let j = c.idx as usize;
            if j <= i {
                return core::ops::ControlFlow::Continue(());
            }
            if i.abs_diff(j) <= 1 {
                return core::ops::ControlFlow::Continue(());
            }
            if check_edge_pair_intersection(ring, i, j, eps) {
                core::ops::ControlFlow::Break(())
            } else {
                core::ops::ControlFlow::<(), ()>::Continue(())
            }
        });
        if found.is_break() {
            return true;
        }
    }
    false
}

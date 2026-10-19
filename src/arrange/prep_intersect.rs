use alloc::vec::Vec;
use geo::Line;
use rstar::{AABB, RTree, RTreeObject};

use crate::validation::sweep::{radix_sort_keys_tls, sortable_u64};

/// Active-set limit for the edge-level spiky-ring sweep; beyond it the
/// input is dense and belongs on the grid/R-tree path (same rationale as
/// the line noder's limit).
const EDGE_SWEEP_ACTIVE_LIMIT: usize = 512;

/// Robust orientation of one line pair against another, batched as a single
/// 4-wide SIMD call: [o(li, lj.start), o(li, lj.end), o(lj, li.start),
/// o(lj, li.end)]. Sign-identical to four separate `orient2d` calls: the
/// hybrid fast+error-bound fallback uses Shewchuk's bound (same as the
/// `robust` crate's own fast path), and near-zero lanes fall back to exact
/// arithmetic. Measured ~2x faster than four separate robust calls in the
/// small-ring sweep that dominates the 1.58M-polygon fast path.
#[inline(always)]
fn orient4(li: &Line<f64>, lj: &Line<f64>) -> [f64; 4] {
    let pa = [li.start, li.start, lj.start, lj.start];
    let pb = [li.end, li.end, lj.end, lj.end];
    let pc = [lj.start, lj.end, li.start, li.end];
    crate::simd::orient2d_batch_4_robust(&pa, &pb, &pc)
}

/// Strict proper-crossing test for two line segments using the batched
/// orientation helper.
#[inline(always)]
fn segments_properly_cross(li: &Line<f64>, lj: &Line<f64>) -> bool {
    let [o1, o2, o3, o4] = orient4(li, lj);
    // The (o1 > 0) != (o2 > 0) form treats
    // Measured: 276/300 real-world repaired components rejected by the --fast gate while the full validator and GEOS accept them (2026-08-03).
    (o1 > 0.0 && o2 < 0.0 || o1 < 0.0 && o2 > 0.0) && (o3 > 0.0 && o4 < 0.0 || o3 < 0.0 && o4 > 0.0)
}

fn quadrant(x: f64, y: f64) -> u8 {
    if x > 0.0 {
        if y >= 0.0 { 0 } else { 1 }
    } else if x < 0.0 {
        if y > 0.0 { 3 } else { 2 }
    } else {
        if y > 0.0 { 0 } else { 2 }
    }
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub(crate) struct MonoChain {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) quad: u8,
    pub(crate) min_x: f64,
    pub(crate) min_y: f64,
    pub(crate) max_x: f64,
    pub(crate) max_y: f64,
    pub(crate) ring_id: u32,
    /// 1e-12 * ring bbox scale, computed once per ring in
    /// build_mono_chains. Used by the same-ring full pair predicate
    /// (the gate must match check_ring_validity's ring-local eps).
    ring_eps: f64,
}

impl MonoChain {
    pub(crate) fn sub_aabb(&self, lines: &[Line<f64>], s: usize, e: usize) -> (f64, f64, f64, f64) {
        let x0 = lines[s].start.x;
        let x1 = lines[e - 1].end.x;
        let y0 = lines[s].start.y;
        let y1 = lines[e - 1].end.y;
        match self.quad {
            0 => (x0, y0, x1, y1),
            1 => (x0, y1, x1, y0),
            2 => (x1, y1, x0, y0),
            3 => (x1, y0, x0, y1),
            _ => (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)),
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn build_mono_chains(lines: &[Line<f64>]) -> (Vec<MonoChain>, (f64, f64, f64, f64)) {
    let n = lines.len();
    if n == 0 {
        return (vec![], (f64::MAX, f64::MIN, f64::MAX, f64::MIN));
    }

    // stamped on its chains (measured 2026-08-07: the old pre-scan cost ~15 us on a 5000-vertex ring).
    // (measured 2026-08-09: one fewer pass on the valid-polygon path).

    let l0 = &lines[0];
    let dx = l0.end.x - l0.start.x;
    let dy = l0.end.y - l0.start.y;
    let mut prev_quad = quadrant(dx, dy);
    let mut start = 0usize;
    let mut min_x = l0.start.x.min(l0.end.x);
    let mut max_x = l0.start.x.max(l0.end.x);
    let mut min_y = l0.start.y.min(l0.end.y);
    let mut max_y = l0.start.y.max(l0.end.y);
    let mut gmin_x = min_x;
    let mut gmax_x = max_x;
    let mut gmin_y = min_y;
    let mut gmax_y = max_y;

    let mut ring_min_x = min_x;
    let mut ring_max_x = max_x;
    let mut ring_min_y = min_y;
    let mut ring_max_y = max_y;
    let mut ring_chain_start = 0usize;
    let mut ring_idx = 0u32;
    let mut chains = Vec::new();

    for (i, line) in lines.iter().enumerate().skip(1) {
        let at_ring_boundary = line.start != lines[i - 1].end;
        min_x = min_x.min(line.start.x).min(line.end.x);
        max_x = max_x.max(line.start.x).max(line.end.x);
        min_y = min_y.min(line.start.y).min(line.end.y);
        max_y = max_y.max(line.start.y).max(line.end.y);
        gmin_x = gmin_x.min(min_x);
        gmax_x = gmax_x.max(max_x);
        gmin_y = gmin_y.min(min_y);
        gmax_y = gmax_y.max(max_y);
        ring_min_x = ring_min_x.min(line.start.x).min(line.end.x);
        ring_max_x = ring_max_x.max(line.start.x).max(line.end.x);
        ring_min_y = ring_min_y.min(line.start.y).min(line.end.y);
        ring_max_y = ring_max_y.max(line.start.y).max(line.end.y);

        let dx = line.end.x - line.start.x;
        let dy = line.end.y - line.start.y;
        let cur_quad = quadrant(dx, dy);
        if at_ring_boundary || cur_quad != prev_quad {
            chains.push(MonoChain {
                start,
                end: i,
                quad: prev_quad,
                min_x,
                min_y,
                max_x,
                max_y,
                ring_id: ring_idx,
                ring_eps: 0.0, // stamped at the ring end
            });
            start = i;
            prev_quad = cur_quad;
            min_x = line.start.x.min(line.end.x);
            max_x = line.start.x.max(line.end.x);
            min_y = line.start.y.min(line.end.y);
            max_y = line.start.y.max(line.end.y);

            if at_ring_boundary {
                let scale = (ring_max_x - ring_min_x)
                    .abs()
                    .max((ring_max_y - ring_min_y).abs())
                    .max(1.0);
                let eps = 1e-12 * scale;
                for mc in chains[ring_chain_start..].iter_mut() {
                    mc.ring_eps = eps;
                }
                ring_idx += 1;
                ring_chain_start = chains.len();
                ring_min_x = min_x;
                ring_max_x = max_x;
                ring_min_y = min_y;
                ring_max_y = max_y;
            }
        }
    }
    chains.push(MonoChain {
        start,
        end: n,
        quad: prev_quad,
        min_x,
        min_y,
        max_x,
        max_y,
        ring_id: ring_idx,
        ring_eps: 0.0, // stamped below
    });
    let scale = (ring_max_x - ring_min_x)
        .abs()
        .max((ring_max_y - ring_min_y).abs())
        .max(1.0);
    let eps = 1e-12 * scale;
    for mc in chains[ring_chain_start..].iter_mut() {
        mc.ring_eps = eps;
    }
    (chains, (gmin_x, gmax_x, gmin_y, gmax_y))
}

/// Fused monotone-chain builder: the gate's plausibility scan feeds it one
/// line at a time, so the chain index is built during the walk the gate
/// already makes instead of a second pass over the collected `lines`.
/// Output is bit-identical to [`build_mono_chains`] for the same input
/// (enforced by the `chain_fusion_equivalence` unit tests).
///
/// Rings are fed in order (`begin_ring` ... lines ... `finish`), which is
/// how every caller already drives [`crate::arrange::ring_is_plausible`]:
/// exterior first, then holes. Ring boundaries are therefore KNOWN here
/// rather than re-detected from `line.start != prev.end`.
pub(crate) struct ChainSink {
    chains: Vec<MonoChain>,
    /// Flat line index of the next push; mirrors the index into the
    /// caller's `lines` vector (both are pushed in lockstep).
    flat: usize,
    start: usize,
    quad: u8,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    chain_open: bool,
    ring_idx: u32,
    ring_chain_start: usize,
    ring_min_x: f64,
    ring_max_x: f64,
    ring_min_y: f64,
    ring_max_y: f64,
    ring_open: bool,
    pending_boundary: bool,
    gmin_x: f64,
    gmax_x: f64,
    gmin_y: f64,
    gmax_y: f64,
    any: bool,
}

impl Default for ChainSink {
    fn default() -> Self {
        ChainSink {
            chains: Vec::new(),
            flat: 0,
            start: 0,
            quad: 0,
            min_x: f64::MAX,
            max_x: f64::MIN,
            min_y: f64::MAX,
            max_y: f64::MIN,
            chain_open: false,
            ring_idx: 0,
            ring_chain_start: 0,
            ring_min_x: f64::MAX,
            ring_max_x: f64::MIN,
            ring_min_y: f64::MAX,
            ring_max_y: f64::MIN,
            ring_open: false,
            pending_boundary: false,
            gmin_x: f64::MAX,
            gmax_x: f64::MIN,
            gmin_y: f64::MAX,
            gmax_y: f64::MIN,
            any: false,
        }
    }
}

impl ChainSink {
    /// Open the next ring. A previously open ring is NOT flushed here: its
    /// close waits for this ring's first line (see `pending_boundary`), so
    /// the fused output matches the reference's boundary handling exactly.
    /// No-op before the first ring.
    pub(crate) fn begin_ring(&mut self) {
        if self.ring_open {
            self.pending_boundary = true;
        }
    }

    /// Feed the ring's next line (the walk's next window).
    pub(crate) fn push_line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        let i = self.flat;
        self.flat += 1;
        let (lm_x, lx_x) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
        let (lm_y, lx_y) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
        if self.pending_boundary {
            self.min_x = self.min_x.min(lm_x);
            self.max_x = self.max_x.max(lx_x);
            self.min_y = self.min_y.min(lm_y);
            self.max_y = self.max_y.max(lx_y);
            self.ring_min_x = self.ring_min_x.min(lm_x);
            self.ring_max_x = self.ring_max_x.max(lx_x);
            self.ring_min_y = self.ring_min_y.min(lm_y);
            self.ring_max_y = self.ring_max_y.max(lx_y);
            self.push_open_chain(i);
            self.stamp_ring_eps();
            self.ring_idx += 1;
            self.ring_chain_start = self.chains.len();
            self.ring_min_x = lm_x;
            self.ring_max_x = lx_x;
            self.ring_min_y = lm_y;
            self.ring_max_y = lx_y;
            self.start = i;
            self.quad = quadrant(x1 - x0, y1 - y0);
            self.min_x = lm_x;
            self.max_x = lx_x;
            self.min_y = lm_y;
            self.max_y = lx_y;
            self.chain_open = true;
            self.ring_open = true;
            self.pending_boundary = false;
            return;
        }
        if !self.ring_open {
            self.ring_open = true;
            self.ring_min_x = lm_x;
            self.ring_max_x = lx_x;
            self.ring_min_y = lm_y;
            self.ring_max_y = lx_y;
        } else {
            self.ring_min_x = self.ring_min_x.min(lm_x);
            self.ring_max_x = self.ring_max_x.max(lx_x);
            self.ring_min_y = self.ring_min_y.min(lm_y);
            self.ring_max_y = self.ring_max_y.max(lx_y);
        }
        if !self.chain_open {
            self.chain_open = true;
            self.start = i;
            self.quad = quadrant(x1 - x0, y1 - y0);
            self.min_x = lm_x;
            self.max_x = lx_x;
            self.min_y = lm_y;
            self.max_y = lx_y;
            return;
        }
        self.min_x = self.min_x.min(lm_x);
        self.max_x = self.max_x.max(lx_x);
        self.min_y = self.min_y.min(lm_y);
        self.max_y = self.max_y.max(lx_y);
        let cur_quad = quadrant(x1 - x0, y1 - y0);
        if cur_quad != self.quad {
            self.chains.push(MonoChain {
                start: self.start,
                end: i,
                quad: self.quad,
                min_x: self.min_x,
                min_y: self.min_y,
                max_x: self.max_x,
                max_y: self.max_y,
                ring_id: self.ring_idx,
                ring_eps: 0.0, // stamped at the ring end
            });
            self.start = i;
            self.quad = cur_quad;
            self.min_x = lm_x;
            self.max_x = lx_x;
            self.min_y = lm_y;
            self.max_y = lx_y;
        }
    }

    /// Push the open chain, ending at line `end`.
    fn push_open_chain(&mut self, end: usize) {
        if self.chain_open {
            self.chains.push(MonoChain {
                start: self.start,
                end,
                quad: self.quad,
                min_x: self.min_x,
                min_y: self.min_y,
                max_x: self.max_x,
                max_y: self.max_y,
                ring_id: self.ring_idx,
                ring_eps: 0.0, // stamped below
            });
            self.chain_open = false;
        }
    }

    /// Stamp the open ring's scale-relative eps on every chain of it, and
    /// fold that ring's bbox into the global envelope.
    ///
    /// The global envelope is the grid path's `global_bbox`. It is the
    /// min/max over every vertex, and every vertex belongs to exactly one
    /// ring bbox, so folding the (complete) ring bbox in here is exactly
    /// equivalent to the per-line accumulation it replaces - with four
    /// min/max per RING instead of four per VERTEX (2026-10-17). Called
    /// once per ring, always with a complete ring bbox: from `push_line`'s
    /// `pending_boundary` arm for the ring being closed, and from
    /// `flush_ring` for the ring being finished.
    fn stamp_ring_eps(&mut self) {
        let scale = (self.ring_max_x - self.ring_min_x)
            .abs()
            .max((self.ring_max_y - self.ring_min_y).abs())
            .max(1.0);
        let eps = 1e-12 * scale;
        for mc in self.chains[self.ring_chain_start..].iter_mut() {
            mc.ring_eps = eps;
        }
        self.gmin_x = self.gmin_x.min(self.ring_min_x);
        self.gmax_x = self.gmax_x.max(self.ring_max_x);
        self.gmin_y = self.gmin_y.min(self.ring_min_y);
        self.gmax_y = self.gmax_y.max(self.ring_max_y);
        self.any = true;
    }

    /// Close the open ring exactly as the reference builder closes it after
    /// its loop: flush the last chain (no foreign line folded in) and stamp
    /// the ring's eps. Tolerates a ring left open by an aborted scan.
    fn flush_ring(&mut self) {
        self.push_open_chain(self.flat);
        self.stamp_ring_eps();
        self.ring_idx += 1;
        self.ring_chain_start = self.chains.len();
        self.ring_open = false;
        self.pending_boundary = false;
    }

    /// Consume the sink: (chains, global bbox) in
    /// [`build_mono_chains`]'s tuple order.
    pub(crate) fn finish(mut self) -> (Vec<MonoChain>, (f64, f64, f64, f64)) {
        if self.ring_open {
            self.flush_ring();
        }
        let bbox = if self.any {
            (self.gmin_x, self.gmax_x, self.gmin_y, self.gmax_y)
        } else {
            (f64::MAX, f64::MIN, f64::MAX, f64::MIN)
        };
        (self.chains, bbox)
    }
}

/// Repeated-vertex class for a NON-ADJACENT pair of edges in the same ring.
///
/// GEOS's `PolygonIntersectionAnalyzer::findInvalidIntersection` treats any
/// same-segString intersection that is not an adjacent-in-ring endpoint
/// touch as `eRingSelfIntersection`, so it needs no duplicate-vertex
/// machinery at all. Our `lean_pair_intersects` classifies an exact shared
/// endpoint as `ambiguous` and *not* intersecting, so the leaf has to close
/// that class itself - otherwise the caller must keep an independent
/// duplicate-vertex table over the whole ring.
///
/// Only three of the four endpoint orientations are tested. The fourth,
/// `a.start == b.end`, is exactly the ring's own closure for the pair
/// (first edge, last edge) - and that pair is deliberately not treated as
/// adjacent here, because the two edges can still overlap collinearly past
/// the shared vertex. Dropping that one orientation is enough:
/// a vertex repeated at positions p < q is caught by
/// - `end == start` on the pair (p-1, q)                    (p >= 1),
/// - `end == end`   on the pair (p-1, q-1) or, for p == 0,   (L-1, q-1),
/// - `start == start` on the pair (p, q).
///
/// Every one of those pairs consists of edges incident to the repeated
/// point, so every one is enumerated.
///
/// Only called when the pair already escalated (`ambiguous`), which every
/// repeated-vertex pair does: the shared point makes one of the four
/// orientations exactly zero, so it can never clear the strong-zone gate.
/// That keeps the three compares off the clean path entirely.
#[inline]
fn shares_repeated_vertex(a: &Line<f64>, b: &Line<f64>) -> bool {
    a.start == b.start || a.end == b.end || a.end == b.start
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn rec_overlaps(
    lines: &[Line<f64>],
    mc1: &MonoChain,
    start0: usize,
    end0: usize,
    mc2: &MonoChain,
    start1: usize,
    end1: usize,
) -> bool {
    if end0 - start0 == 1 && end1 - start1 == 1 {
        let i = start0;
        let j = start1;
        if i == j {
            return false;
        }
        // Closing pair is NOT skipped: shared vertex 0 aside
        if mc1.ring_id == mc2.ring_id && (j == i + 1 || j + 1 == i) {
            return false;
        }
        let li = &lines[i];
        let lj = &lines[j];
        if segments_properly_cross(li, lj) {
            return true;
        }
        // polygon the exit validator would reject (2026-08-07).
        if mc1.ring_id == mc2.ring_id {
            let eps = mc1.ring_eps;
            let mut ambiguous = false;
            if crate::validation::edges::lean_pair_intersects(
                li.start,
                li.end,
                lj.start,
                lj.end,
                eps,
                &mut ambiguous,
            ) {
                return true;
            }
            // This replaces the per-ring duplicate-vertex hash table the caller used to carry (2026-10-17) - same verdicts, no table.
            if ambiguous && shares_repeated_vertex(li, lj) {
                return true;
            }
        }
        return false;
    }

    let (minx0, miny0, maxx0, maxy0) = mc1.sub_aabb(lines, start0, end0);
    let (minx1, miny1, maxx1, maxy1) = mc2.sub_aabb(lines, start1, end1);
    if minx0 > maxx1 + 1e-12
        || maxx0 < minx1 - 1e-12
        || miny0 > maxy1 + 1e-12
        || maxy0 < miny1 - 1e-12
    {
        return false;
    }

    if (end0 - start0) >= (end1 - start1) {
        let mid = (start0 + end0) / 2;
        if start0 < mid && rec_overlaps(lines, mc1, start0, mid, mc2, start1, end1) {
            return true;
        }
        if mid < end0 {
            return rec_overlaps(lines, mc1, mid, end0, mc2, start1, end1);
        }
    } else {
        let mid = (start1 + end1) / 2;
        if start1 < mid && rec_overlaps(lines, mc1, start0, end0, mc2, start1, mid) {
            return true;
        }
        if mid < end1 {
            return rec_overlaps(lines, mc1, start0, end0, mc2, mid, end1);
        }
    }
    false
}

fn compute_overlaps(lines: &[Line<f64>], mc1: &MonoChain, mc2: &MonoChain) -> bool {
    rec_overlaps(lines, mc1, mc1.start, mc1.end, mc2, mc2.start, mc2.end)
}

/// Small-ring O(n²) pairwise proper-crossing sweep.
///
/// Replicates the exact predicate of the monotone-chain leaf
/// `rec_overlaps` for `n <= SMALL_RING_LINES` lines:
/// - strict proper crossing: `(o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0)`
///   using the same `orient2d` (Shewchuk via crate::orient)
/// - same-ring adjacency skip: edges i and j are adjacent if `j == i + 1`
///   or `j + 1 == i`, and the closing pair (first, last) is skipped -
///   both mirror rec_overlaps lines 148-156
/// - different rings always compare (ring boundary = segment whose start
///   != previous segment's end, same as build_mono_chains)
///
/// Allocates nothing: `ring_of[i]` is a stack array sized by the caller's
/// `SMALL_RING_LINES` bound.
pub fn has_no_intersections_small(lines: &[Line<f64>]) -> bool {
    let n = lines.len();
    debug_assert!(n <= crate::core::SMALL_RING_LINES);
    if n < 2 {
        return true;
    }
    let mut ring_of = [0u32; crate::core::SMALL_RING_LINES + 1];
    let mut nrings = 1u32;
    for i in 1..n {
        if lines[i].start != lines[i - 1].end {
            nrings += 1;
        }
        ring_of[i] = nrings - 1;
    }
    // versions cost 5 small allocations per gate call on the 95.6%
    let mut min_x = [f64::MAX; crate::core::SMALL_RING_LINES];
    let mut max_x = [f64::MIN; crate::core::SMALL_RING_LINES];
    let mut min_y = [f64::MAX; crate::core::SMALL_RING_LINES];
    let mut max_y = [f64::MIN; crate::core::SMALL_RING_LINES];
    for (i, l) in lines.iter().enumerate() {
        let r = ring_of[i] as usize;
        min_x[r] = min_x[r].min(l.start.x.min(l.end.x));
        max_x[r] = max_x[r].max(l.start.x.max(l.end.x));
        min_y[r] = min_y[r].min(l.start.y.min(l.end.y));
        max_y[r] = max_y[r].max(l.start.y.max(l.end.y));
    }
    let mut eps_by_ring = [0.0f64; crate::core::SMALL_RING_LINES];
    for r in 0..nrings as usize {
        let scale = (max_x[r] - min_x[r])
            .abs()
            .max((max_y[r] - min_y[r]).abs())
            .max(1.0);
        eps_by_ring[r] = 1e-12 * scale;
    }

    for i in 0..n {
        let li = &lines[i];
        let ri = ring_of[i] as usize;
        for j in (i + 1)..n {
            if ri == ring_of[j] as usize {
                // The closing pair (first vs last edge) is NOT skipped: the two share vertex 0
                if j == i + 1 || j + 1 == i {
                    continue;
                }
            }
            let lj = &lines[j];
            if segments_properly_cross(li, lj) {
                return false;
            }
            // (proper crossing + eps-collinear overlap + vertex-on-edge T-junction) with the ring's own eps - identical to check_ring_validity's sweep (2026-08-07).
            if ri == ring_of[j] as usize {
                let eps = eps_by_ring[ri];
                let mut ambiguous = false;
                if crate::validation::edges::lean_pair_intersects(
                    li.start,
                    li.end,
                    lj.start,
                    lj.end,
                    eps,
                    &mut ambiguous,
                ) {
                    return false;
                }
                if ambiguous && shares_repeated_vertex(li, lj) {
                    return false;
                }
            }
        }
    }
    true
}

pub(crate) struct ChainEnv {
    idx: usize,
    env: AABB<[f64; 2]>,
}
impl RTreeObject for ChainEnv {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn has_no_intersections(lines: &[Line<f64>]) -> bool {
    has_no_intersections_tuned(lines, &crate::core::Tuning::default())
}

/// Tuned variant of [`has_no_intersections`]: the repair pipeline threads
/// the caller's [`crate::core::Tuning`] through the small-ring dispatch.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn has_no_intersections_tuned(
    lines: &[Line<f64>],
    tuning: &crate::core::Tuning,
) -> bool {
    let n = lines.len();
    if n == 0 {
        return true;
    }
    for line in lines {
        if !line.start.x.is_finite()
            || !line.start.y.is_finite()
            || !line.end.x.is_finite()
            || !line.end.y.is_finite()
        {
            return false;
        }
    }
    has_no_intersections_nan_ok_tuned(lines, tuning)
}

/// `has_no_intersections` for line arrays whose finiteness was already
/// verified by the caller (the fast-path gate's `ring_is_plausible` scan
/// checks every coordinate before collecting the lines). Saves one full
/// pass over the lines on the valid-polygon path (measured 2026-08-09:
/// the NaN scan is a redundant ~1-2 us on a 5000-vertex ring - and one
/// less pass of memory traffic on the bandwidth-bound parallel rows).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn has_no_intersections_nan_ok(lines: &[Line<f64>]) -> bool {
    has_no_intersections_nan_ok_tuned(lines, &crate::core::Tuning::default())
}

/// Tuned variant of [`has_no_intersections_nan_ok`].
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn has_no_intersections_nan_ok_tuned(
    lines: &[Line<f64>],
    tuning: &crate::core::Tuning,
) -> bool {
    let n = lines.len();
    if n == 0 {
        return true;
    }

    // 1.295 µs/poly there vs ~0.1 µs for the pairwise sweep .
    if n <= crate::core::SMALL_RING_LINES.min(tuning.small_ring_lines) {
        return has_no_intersections_small(lines);
    }

    let (chains, global_bbox) = build_mono_chains(lines);
    has_no_intersections_from_chains(lines, &chains, global_bbox, tuning)
}

/// The chain-index half of [`has_no_intersections_nan_ok_tuned`], split out
/// so a caller that already owns a chain index - the gate builds it inside
/// its plausibility scan via [`ChainSink`] instead of making a second pass
/// over the collected lines - pays no extra traversal. The verdict must be
/// identical whichever way `chains` was produced (enforced by the
/// `chain_fusion_equivalence` unit tests).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn has_no_intersections_from_chains(
    lines: &[Line<f64>],
    chains: &[MonoChain],
    global_bbox: (f64, f64, f64, f64),
    tuning: &crate::core::Tuning,
) -> bool {
    if lines.is_empty() {
        return true;
    }
    if lines.len() <= crate::core::SMALL_RING_LINES.min(tuning.small_ring_lines) {
        return has_no_intersections_small(lines);
    }
    debug_assert_eq!(
        chains.last().map(|c| c.end),
        Some(lines.len()),
        "chain index must cover every collected line"
    );
    if chains.is_empty() {
        return true;
    }

    // pairs directly (measured: star poly 500v = 63,603 grid cell-pair tests vs 5,272 sweep tests, 12x).
    if nc_is_spiky(chains, tuning)
        && let Some(result) = has_no_intersections_edge_sweep(lines)
    {
        return result;
    }

    let grid_result = has_no_intersections_grid(chains, lines, global_bbox);
    if let Some(result) = grid_result {
        return result;
    }

    let envs: Vec<ChainEnv> = chains
        .iter()
        .enumerate()
        .map(|(i, mc)| ChainEnv {
            idx: i,
            env: AABB::from_corners([mc.min_x, mc.min_y], [mc.max_x, mc.max_y]),
        })
        .collect();
    let tree = RTree::bulk_load(envs);
    let nc = chains.len();

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        let do_parallel = nc >= 200;
        if do_parallel {
            use core::ops::ControlFlow;
            use core::sync::atomic::Ordering;
            use rayon::prelude::*;
            let found = std::sync::atomic::AtomicBool::new(false);
            (0..nc).into_par_iter().for_each(|i| {
                if found.load(Ordering::Acquire) {
                    return;
                }
                let mc1 = &chains[i];
                let q = AABB::from_corners([mc1.min_x, mc1.min_y], [mc1.max_x, mc1.max_y]);
                let res = tree.locate_in_envelope_intersecting_int(q, |c| {
                    if found.load(Ordering::Acquire) {
                        return ControlFlow::Break(());
                    }
                    let j = c.idx;
                    if j <= i {
                        return ControlFlow::Continue(());
                    }
                    if compute_overlaps(lines, mc1, &chains[j]) {
                        found.store(true, Ordering::Release);
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                });
                if res.is_break() && !found.load(Ordering::Acquire) {
                    found.store(true, Ordering::Release);
                }
            });
            return !found.load(Ordering::Acquire);
        }
    }

    use core::ops::ControlFlow;
    for i in 0..nc {
        let mc1 = &chains[i];
        let q = AABB::from_corners([mc1.min_x, mc1.min_y], [mc1.max_x, mc1.max_y]);
        let result = tree.locate_in_envelope_intersecting_int(q, |c| {
            let j = c.idx;
            if j <= i {
                return ControlFlow::Continue(());
            }
            if compute_overlaps(lines, mc1, &chains[j]) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        if result.is_break() {
            return false;
        }
    }
    true
}

/// Spiky-chain detector: the fraction of chains that hold exactly one edge.
/// Star/radial rings flip quadrant at nearly every vertex, so nearly every
/// chain is a single edge; smooth rings produce few long chains. `frac` is
/// the Tuning threshold (default 0.8). Floor of 128 chains: below that the
/// grid is small and cheap regardless of shape (measured: star poly 100v,
/// 92 chains - the sweep's setup cost more than the 10x10 grid it replaced),
/// so the sweep only engages where the grid's pair explosion is real.
fn nc_is_spiky(chains: &[MonoChain], tuning: &crate::core::Tuning) -> bool {
    if chains.len() < 128 || chains.len() > tuning.spiky_max_chains {
        return false;
    }
    let ones = chains.iter().filter(|mc| mc.end - mc.start == 1).count();
    (ones as f64) >= tuning.spiky_chain_frac * (chains.len() as f64)
}

/// Edge-level x-radix sweep for spiky rings: sort edges by padded lo_x, keep
/// an active set of x-overlapping edges, y-gate each candidate pair, then run
/// the SAME predicate chain as rec_overlaps' leaf. Returns None when the
/// active set explodes (dense inputs belong on the R-tree path).
///
/// Pair semantics mirror rec_overlaps exactly:
/// - proper crossing via orient4/segments_properly_cross,
/// - same-ring pairs escalate to lean_pair_intersects with the ring's own
///   eps (proper crossing + eps-collinear overlap + vertex-on-edge),
/// - same-ring adjacent edges skipped, closing pair NOT skipped,
/// - cross-ring pairs: proper crossings only.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn has_no_intersections_edge_sweep(lines: &[Line<f64>]) -> Option<bool> {
    let n = lines.len();

    let mut ring_of = vec![0u32; n];
    let mut nrings = 1u32;
    for i in 1..n {
        if lines[i].start != lines[i - 1].end {
            nrings += 1;
        }
        ring_of[i] = nrings - 1;
    }
    let mut min_x = vec![f64::MAX; nrings as usize];
    let mut max_x = vec![f64::MIN; nrings as usize];
    let mut min_y = vec![f64::MAX; nrings as usize];
    let mut max_y = vec![f64::MIN; nrings as usize];
    for (i, l) in lines.iter().enumerate() {
        let r = ring_of[i] as usize;
        min_x[r] = min_x[r].min(l.start.x.min(l.end.x));
        max_x[r] = max_x[r].max(l.start.x.max(l.end.x));
        min_y[r] = min_y[r].min(l.start.y.min(l.end.y));
        max_y[r] = max_y[r].max(l.start.y.max(l.end.y));
    }
    let ring_eps: Vec<f64> = (0..nrings as usize)
        .map(|r| {
            let scale = (max_x[r] - min_x[r])
                .abs()
                .max((max_y[r] - min_y[r]).abs())
                .max(1.0);
            1e-12 * scale
        })
        .collect();

    let mut spans: Vec<[f64; 4]> = Vec::with_capacity(n);
    let mut keys: Vec<u64> = Vec::with_capacity(n);
    let mut order: Vec<u32> = Vec::with_capacity(n);
    for l in lines {
        let (lo_x, hi_x) = if l.start.x < l.end.x {
            (l.start.x, l.end.x)
        } else {
            (l.end.x, l.start.x)
        };
        let (lo_y, hi_y) = if l.start.y < l.end.y {
            (l.start.y, l.end.y)
        } else {
            (l.end.y, l.start.y)
        };
        let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
        keys.push(sortable_u64(lo_x - ext));
        order.push(0);
        spans.push([lo_x - ext, hi_x + ext, lo_y - ext, hi_y + ext]);
    }

    let mut sorted = true;
    for i in 1..n {
        if keys[i] < keys[i - 1] {
            sorted = false;
            break;
        }
    }
    if sorted {
        for (i, o) in order.iter_mut().enumerate() {
            *o = i as u32;
        }
    } else {
        radix_sort_keys_tls(&mut keys, &mut order);
    }

    let mut active: Vec<u32> = Vec::with_capacity(64);
    for &oi in &order {
        let j = oi as usize;
        if active.len() > EDGE_SWEEP_ACTIVE_LIMIT {
            return None;
        }
        active.retain(|&p| spans[p as usize][1] >= spans[j][0]);
        let sj = &spans[j];
        let lj = &lines[j];
        let rj = ring_of[j];
        for &p in &active {
            let t = &spans[p as usize];
            if t[3] < sj[2] || t[2] > sj[3] {
                continue;
            }
            let i = p as usize;
            let li = &lines[i];
            let ri = ring_of[i];
            if ri == rj && (j == i + 1 || j + 1 == i) {
                continue;
            }
            if segments_properly_cross(li, lj) {
                return Some(false);
            }
            if ri == rj {
                let eps = ring_eps[ri as usize];
                let mut ambiguous = false;
                if crate::validation::edges::lean_pair_intersects(
                    li.start,
                    li.end,
                    lj.start,
                    lj.end,
                    eps,
                    &mut ambiguous,
                ) {
                    return Some(false);
                }
                if ambiguous && shares_repeated_vertex(li, lj) {
                    return Some(false);
                }
            }
        }
        active.push(oi);
    }
    Some(true)
}

/// Fast grid path for `has_no_intersections`. Returns `None` if the grid is
/// too dense, triggering the R-tree fallback. The global bbox comes from
/// `build_mono_chains` (no re-scan of the chains).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn has_no_intersections_grid(
    chains: &[MonoChain],
    lines: &[Line<f64>],
    (min_x, max_x, min_y, max_y): (f64, f64, f64, f64),
) -> Option<bool> {
    let nc = chains.len();
    let scale = (max_x - min_x).max(max_y - min_y);
    if scale <= 0.0 {
        return Some(true);
    }

    let cell_size = scale / (nc as f64).sqrt().ceil();
    let cell_size = cell_size.max(f64::EPSILON);
    let nx = ((max_x - min_x) / cell_size).ceil() as usize;
    let ny = ((max_y - min_y) / cell_size).ceil() as usize;
    let grid_cells = nx.max(1) * ny.max(1);

    let mut cell_chains: Vec<Vec<usize>> = vec![Vec::new(); grid_cells];
    for (i, mc) in chains.iter().enumerate() {
        let x0 = ((mc.min_x - min_x) / cell_size) as isize;
        let x1 = ((mc.max_x - min_x) / cell_size) as isize;
        let y0 = ((mc.min_y - min_y) / cell_size) as isize;
        let y1 = ((mc.max_y - min_y) / cell_size) as isize;
        for cy in y0.max(0)..(y1 + 1).min(ny as isize) {
            for cx in x0.max(0)..(x1 + 1).min(nx as isize) {
                let cell = &mut cell_chains[cy as usize * nx + cx as usize];
                cell.push(i);
                if cell.len() > 64 {
                    return None; // too dense → fall back to R-tree
                }
            }
        }
    }

    // and a 5000-vertex circle's chain pairs were tested 4-9x
    // A mask of j >= 64 would silently skip pairs - never dedup by global index without this guard.
    let mut tested: Vec<u64> = Vec::new();
    if nc <= 64 {
        tested = vec![0; nc.max(1)];
    }
    for cell in &cell_chains {
        for ii in 0..cell.len() {
            let i = cell[ii];
            let mc1 = &chains[i];
            for &j in &cell[ii + 1..] {
                if !tested.is_empty() {
                    if tested[i] >> j & 1 == 1 {
                        continue;
                    }
                    tested[i] |= 1 << j;
                }
                if compute_overlaps(lines, mc1, &chains[j]) {
                    return Some(false);
                }
            }
        }
    }
    Some(true)
}

#[cfg(test)]
mod chain_fusion_tests {
    use super::*;
    use crate::arrange::{GateAccum, ring_is_plausible};
    use geo::{LineString, LinesIter, Polygon};

    /// One fused walk (lines + chain index + envelope in a single pass),
    /// next to the reference outputs computed from the SAME collected lines.
    /// Coordinates in the fixtures avoid signed zero: IEEE min/max returns
    /// the second operand on a tie, so -0.0 vs 0.0 can make two correct
    /// accumulators differ in representation only. `nz` normalizes that so
    /// the assertions compare semantics, not tie-break order.
    struct Snapshot {
        ok: bool,
        lines: Vec<Line<f64>>,
        chains: Vec<MonoChain>,
        fused_bbox: (f64, f64, f64, f64),
        walk_bbox: (f64, f64, f64, f64),
        ref_chains: Vec<MonoChain>,
        ref_bbox: (f64, f64, f64, f64),
    }

    /// Drive `ring_is_plausible` exactly the way the fast-path gate does:
    /// exterior first, then holes, with the line, chain and bbox
    /// accumulators live.
    fn walk(poly: &Polygon<f64>) -> Snapshot {
        let mut lines = Vec::new();
        let mut sink = ChainSink::default();
        let mut bbox = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        let mut acc = GateAccum {
            lines: Some(&mut lines),
            chains: Some(&mut sink),
            bbox: Some(&mut bbox),
            extremal: None,
        };
        let mut ok = ring_is_plausible(poly.exterior(), &mut acc);
        for hole in poly.interiors() {
            if !ok {
                break;
            }
            ok = ring_is_plausible(hole, &mut acc);
        }
        let (chains, fused_bbox) = sink.finish();
        let (ref_chains, ref_bbox) = build_mono_chains(&lines);
        Snapshot {
            ok,
            lines,
            chains,
            fused_bbox,
            walk_bbox: bbox,
            ref_chains,
            ref_bbox,
        }
    }

    fn nz(v: f64) -> f64 {
        if v == 0.0 { 0.0 } else { v }
    }

    fn nz_bbox(b: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
        (nz(b.0), nz(b.1), nz(b.2), nz(b.3))
    }

    fn nz_chains(c: &[MonoChain]) -> Vec<MonoChain> {
        c.iter()
            .map(|m| MonoChain {
                start: m.start,
                end: m.end,
                quad: m.quad,
                min_x: nz(m.min_x),
                min_y: nz(m.min_y),
                max_x: nz(m.max_x),
                max_y: nz(m.max_y),
                ring_id: m.ring_id,
                ring_eps: m.ring_eps,
            })
            .collect()
    }

    /// The invariant the gate's mid-size branch rests on: whatever the
    /// fused walk produced, `build_mono_chains` would have produced it too,
    /// from the same lines, with the same envelope. On a completed walk the
    /// line collection must also be the polygon's full line set.
    fn assert_equivalent(poly: &Polygon<f64>) -> Snapshot {
        let s = walk(poly);
        assert_eq!(
            nz_chains(&s.chains),
            nz_chains(&s.ref_chains),
            "fused chain index must equal build_mono_chains"
        );
        assert_eq!(
            nz_bbox(s.fused_bbox),
            nz_bbox(s.ref_bbox),
            "fused envelope must equal build_mono_chains's"
        );
        if s.ok {
            assert_eq!(
                nz_bbox(s.fused_bbox),
                nz_bbox(s.walk_bbox),
                "fused envelope must equal the walk's bbox accumulator"
            );
            let all: Vec<Line<f64>> = poly.lines_iter().collect();
            assert_eq!(s.lines, all, "a completed walk collects every line");
        } else {
            assert!(
                s.fused_bbox.0 <= s.walk_bbox.0
                    && s.fused_bbox.1 >= s.walk_bbox.1
                    && s.fused_bbox.2 <= s.walk_bbox.2
                    && s.fused_bbox.3 >= s.walk_bbox.3,
                "fused envelope must contain the walk's accumulator"
            );
        }
        s
    }

    /// The sweep verdict must not depend on who built the chain index.
    fn assert_same_verdict(poly: &Polygon<f64>) -> Snapshot {
        let s = assert_equivalent(poly);
        if !s.ok {
            return s;
        }
        let tuning = crate::core::Tuning::default();
        assert_eq!(
            has_no_intersections_nan_ok_tuned(&s.lines, &tuning),
            has_no_intersections_from_chains(&s.lines, &s.chains, s.fused_bbox, &tuning),
            "verdict must not depend on who built the chains"
        );
        s
    }

    fn circle(n: usize, cx: f64, cy: f64, r: f64) -> LineString<f64> {
        let mut pts: Vec<geo::Coord<f64>> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * (i as f64) / (n as f64);
                geo::Coord {
                    x: cx + r * a.cos(),
                    y: cy + r * a.sin(),
                }
            })
            .collect();
        pts.push(pts[0]);
        LineString::new(pts)
    }

    fn polygon_with_holes(shell: LineString<f64>, holes: Vec<LineString<f64>>) -> Polygon<f64> {
        Polygon::new(shell, holes)
    }

    #[test]
    fn circle_single_ring() {
        assert_same_verdict(&polygon_with_holes(circle(64, 10.0, 20.0, 5.0), vec![]));
    }

    #[test]
    fn shell_plus_hole_travels_two_rings() {
        // Two rings: ring_id and the per-ring eps stamp must line up with the reference's ring-boundary re-detection.
        let poly = polygon_with_holes(
            circle(64, 0.0, 0.0, 100.0),
            vec![circle(48, 3.0, 7.0, 20.0)],
        );
        let s = assert_same_verdict(&poly);
        assert!(s.ok, "concentric-ish rings with a gap are plausible");
        assert!(s.chains.iter().any(|c| c.ring_id == 1), "hole chains exist");
    }

    #[test]
    fn corner_heavy_ring_breaks_chains() {
        let mut pts = vec![geo::Coord { x: 1.0, y: 1.0 }];
        let mut t = 2.0;
        for i in 0..40 {
            pts.push(geo::Coord {
                x: t,
                y: 1.0 + (i as f64),
            });
            pts.push(geo::Coord {
                x: t + 0.5,
                y: 1.0 + (i as f64),
            });
            t += 0.5;
        }
        pts.push(pts[0]);
        assert_same_verdict(&polygon_with_holes(LineString::new(pts), vec![]));
    }

    #[test]
    fn star_many_quadrant_flips() {
        let mut pts: Vec<geo::Coord<f64>> = Vec::new();
        for i in 0..120 {
            let a = std::f64::consts::TAU * (i as f64) / 120.0;
            let r = if i % 2 == 0 { 50.0 } else { 20.0 };
            pts.push(geo::Coord {
                x: 5.0 + r * a.cos(),
                y: -3.0 + r * a.sin(),
            });
        }
        pts.push(pts[0]);
        assert_same_verdict(&polygon_with_holes(LineString::new(pts), vec![]));
    }

    #[test]
    fn bowtie_reports_the_same_crossing_from_either_index() {
        let mut pts: Vec<geo::Coord<f64>> = Vec::new();
        for i in 0..50 {
            pts.push(geo::Coord {
                x: (i as f64) * 0.3,
                y: (i as f64) * 0.3,
            });
        }
        for i in 0..50 {
            pts.push(geo::Coord {
                x: (50.0 - i as f64) * 0.3,
                y: (i as f64) * 0.3 + 0.07,
            });
        }
        pts.push(pts[0]);
        let poly = polygon_with_holes(LineString::new(pts), vec![]);
        let s = assert_same_verdict(&poly);
        assert!(s.ok, "a bowtie is still a plausible ring");
        assert!(
            !has_no_intersections_nan_ok_tuned(&s.lines, &crate::core::Tuning::default()),
            "the bowtie must be reported as intersecting"
        );
    }

    #[test]
    fn tiny_ring_below_the_fuse_threshold() {
        // a 4-vertex ring must still index identically.
        assert_same_verdict(&polygon_with_holes(
            LineString::new(vec![
                geo::Coord { x: 2.0, y: 2.0 },
                geo::Coord { x: 8.0, y: 2.0 },
                geo::Coord { x: 8.0, y: 9.0 },
                geo::Coord { x: 2.0, y: 9.0 },
                geo::Coord { x: 2.0, y: 2.0 },
            ]),
            vec![],
        ));
    }

    #[test]
    fn degenerate_point_ring_aborts_cleanly() {
        let pts = vec![geo::Coord { x: 4.0, y: 4.0 }; 40];
        let poly = polygon_with_holes(LineString::new(pts), vec![]);
        let s = assert_equivalent(&poly);
        assert!(!s.ok, "a collapsed ring is not plausible");
        assert!(s.chains.is_empty());
        assert!(s.lines.is_empty());
    }

    #[test]
    fn aborted_walk_indexes_only_the_prefix_it_collected() {
        // Adjacent duplicate at window 20: the walk stops mid-ring
        let mut pts: Vec<geo::Coord<f64>> = (0..40)
            .map(|i| geo::Coord {
                x: 10.0 + (i as f64) * 0.7,
                y: 12.0 - (i as f64) * 0.3,
            })
            .collect();
        pts[21] = pts[20];
        pts.push(pts[0]);
        let poly = polygon_with_holes(LineString::new(pts), vec![]);
        let s = assert_equivalent(&poly);
        assert!(!s.ok, "an adjacent duplicate fails the walk");
        assert!(!s.lines.is_empty(), "the prefix before the failure is kept");
        assert_eq!(s.chains.last().map(|c| c.end), Some(s.lines.len()));
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn fused_index_and_verdict_match_the_reference(
                pts in proptest::collection::vec(
                    (-500.0f64..500.0, -500.0f64..500.0),
                    1..220,
                ),
                hole_pts in proptest::option::of(proptest::collection::vec(
                    (-500.0f64..500.0, -500.0f64..500.0),
                    1..80,
                )),
            ) {
                let mk = |mut p: Vec<(f64, f64)>| {
                    // Pin -0.0 to +0.0: the equivalence below compares bit patterns, and IEEE min/max breaks ties by operand order.
                    for c in p.iter_mut() {
                        if c.0 == 0.0 {
                            c.0 = 0.0;
                        }
                        if c.1 == 0.0 {
                            c.1 = 0.0;
                        }
                    }
                    let mut coords: Vec<geo::Coord<f64>> = p
                        .iter()
                        .map(|&(x, y)| geo::Coord { x, y })
                        .collect();
                    if coords.len() < 2 {
                        coords.push(geo::Coord { x: 1.0, y: 1.0 });
                        coords.push(geo::Coord { x: 2.0, y: 3.0 });
                    }
                    coords.push(coords[0]);
                    LineString::new(coords)
                };
                let poly = Polygon::new(
                    mk(pts),
                    hole_pts.map(mk).into_iter().collect(),
                );
                let s = assert_same_verdict(&poly);
                prop_assert!(
                    s.chains.last().is_none_or(|c| c.end <= s.lines.len())
                );
            }
        }
    }
}

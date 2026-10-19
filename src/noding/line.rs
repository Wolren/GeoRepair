//! The greedy filter drops data at 1892 us on 5000 vertices. Its detection uses the
//! validator tolerances: 1e-12 * bbox scale, a 32-ulp bound, and 1e-12 * len squared.
//! Collinear families above 64 members leave the 2-D sweep.

use alloc::vec;
use alloc::vec::Vec;

use geo::Coord;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::validation::impls::segments_collinear_overlap;
use crate::validation::sweep::{SegGrid, radix_sort_keys_tls, sortable_u64};

mod classify;
mod pieces;
const NO_FAMILY: u32 = u32::MAX;
const NO_SEG: u32 = u32::MAX;
/// Active-set limit for the 2-D sweep; beyond it the noder bails to the caller's
/// fallback (a dense active set means the retain loop is O(n²) anyway - the family pass
const SWEEP_ACTIVE_LIMIT: usize = 512;
/// Families larger than this are removed from the 2-D sweep (their members span long
/// x-ranges and would pin the active set open); cross-family pairs are tested directly
/// instead - pruned by line geometry, see `LineNoder::test_family_pair`.
const FAMILY_IN_SWEEP_LIMIT: usize = 64;
/// Family-pair pruning (test_family_pair): |sin| below this treats the two reference
/// lines as parallel.
const FAMILY_PARALLEL_SIN: f64 = 1e-12;
/// Family-pair pruning: |sin| below this keeps the exhaustive pairwise scan - the
/// crossing point P of near-parallel lines drifts by (32-ulp offset deviation)/sin
/// along the lines, which swamps the span filter's precision budget.
const FAMILY_SHALLOW_SIN: f64 = 1e-6;
/// The validator flags collinear overlap only within 64 eps x len (~1.4e-14 x scale)
/// and vertex-on-edge within 1e-12 x len. Family members deviate from their reference
/// line by at most the 32-ulp offset merge (~7e-15 x scale).
const FAMILY_PARALLEL_SKIP_DIST: f64 = 1e-9;
/// Covers the worst legal drift of a true member-pair crossing from the reference
/// crossing point ((32-ulp deviation)/sin at the FAMILY_SHALLOW_SIN floor ~ 7e-9 x
/// scale) with margin; eps is 1e-12 x scale, so the pad is 1e-7 x scale.
const FAMILY_SPAN_PAD_EPS: f64 = 1e5;
/// Crossing-only fast path: per-pair classify budget (factor x n plus a floor).
const CROSSING_ONLY_PAIR_FACTOR: usize = 32;
const CROSSING_ONLY_PAIR_FLOOR: usize = 1024;

/// Measured 2026-10-17: it lost 30% on the ringing ladder, 90 to 116 us at 1000
/// vertices, against 5a07c75.
const CHAIN_INDEX_WORK_LIMIT: usize = 0;
/// eps-class guard scan budget (iterations over x-sorted points, factor x points plus a
/// floor).
const EPS_CLASS_SCAN_FACTOR: usize = 64;
const EPS_CLASS_SCAN_FLOOR: usize = 4096;

/// A chain the validator rejects on sight: fewer than 2 points (RingTooFewPoints) or an
/// adjacent-duplicate pair (RepeatedPoint).
fn chain_degenerate(chain: &[Coord<f64>]) -> bool {
    chain.len() < 2 || chain.windows(2).any(|w| w[0] == w[1])
}

/// Outcome of the lean per-pair test (mirrors the validator's predicate chain: fast-FP
/// first, robust escalation, collinear, vertex-on-edge, shared endpoint).
#[derive(Clone, Copy, Debug)]
enum Hit {
    /// No intersection per the validator's predicates.
    None,
    /// Segments share an endpoint exactly (vertex revisit).
    Shared,
    /// Proper crossing; the point is the DD intersection, shared by both segments.
    Cross(Coord<f64>),
    /// Collinear overlap (adaptive gate, t-overlap beyond eps).
    Collinear,
    /// The returned vertex lies strictly on the other segment's interior (segment-local
    /// tolerance).
    VertexOnEdge(Coord<f64>),
}

struct NodeEnt {
    seg: u32,
    pt: Coord<f64>,
}

pub(crate) struct LineNoder<'a> {
    coords: &'a [Coord<f64>],
    n: usize,
    scale: f64,
    eps: f64,
    a: Vec<Coord<f64>>,
    b: Vec<Coord<f64>>,
    lo_x: Vec<f64>,
    hi_x: Vec<f64>,
    lo_y: Vec<f64>,
    hi_y: Vec<f64>,
    family: Vec<u32>,
    family_size: Vec<usize>,
    nodes: Vec<NodeEnt>,
    /// Set when a near-collinear (non-exact) overlap pair was noded; the tolerant
    /// piece-dedup sweep then runs to drop coincident residuals.
    near_collinear: bool,
    splits: Vec<Vec<Coord<f64>>>,
    /// Cluster canonical lookup for piece-endpoint snapping.
    canon_map: FxHashMap<(u64, u64), Coord<f64>>,
    /// Members of each removed family (size > FAMILY_IN_SWEEP_LIMIT), populated in
    /// `sweep_pass` and consumed by `test_family_pair`.
    family_members: FxHashMap<u32, Vec<u32>>,
}

/// Node a non-simple line into simple chains.
pub(crate) fn node_line(coords: &[Coord<f64>]) -> Option<Vec<Vec<Coord<f64>>>> {
    let n = coords.len() - 1;
    if n < 2 {
        return Some(vec![coords.to_vec()]);
    }
    let mut ln = LineNoder::new(coords);
    ln.run()
}

/// Shared budget/verdict state for the chain-index pair enumeration.
struct ScanState {
    bail: bool,
    calls: usize,
    limit: usize,
}

impl<'a> LineNoder<'a> {
    pub(super) fn new(coords: &'a [Coord<f64>]) -> Self {
        let n = coords.len() - 1;
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for c in coords {
            min_x = min_x.min(c.x);
            max_x = max_x.max(c.x);
            min_y = min_y.min(c.y);
            max_y = max_y.max(c.y);
        }
        let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
        let eps = 1e-12 * scale;
        let mut a = Vec::with_capacity(n);
        let mut b = Vec::with_capacity(n);
        let mut lo_x = Vec::with_capacity(n);
        let mut hi_x = Vec::with_capacity(n);
        let mut lo_y = Vec::with_capacity(n);
        let mut hi_y = Vec::with_capacity(n);
        for i in 0..n {
            let p = coords[i];
            let q = coords[i + 1];
            a.push(p);
            b.push(q);
            lo_x.push(p.x.min(q.x));
            hi_x.push(p.x.max(q.x));
            lo_y.push(p.y.min(q.y));
            hi_y.push(p.y.max(q.y));
        }
        LineNoder {
            coords,
            n,
            scale,
            eps,
            a,
            b,
            lo_x,
            hi_x,
            lo_y,
            hi_y,
            family: vec![NO_FAMILY; n],
            family_size: Vec::new(),
            nodes: Vec::new(),
            near_collinear: false,
            splits: vec![Vec::new(); n],
            canon_map: FxHashMap::default(),
            family_members: FxHashMap::default(),
        }
    }

    fn closed(&self) -> bool {
        self.coords[0] == self.coords[self.n]
    }

    fn run(&mut self) -> Option<Vec<Vec<Coord<f64>>>> {
        if self.n >= 32 && self.revisit_dominated() {
            return None;
        }
        // both of its segments (an unplaceable node bails to the general path - the
        // fuzz wkb seed 17790c39 dropped-split class) Re-validating every chain here
        // cost ~40% on the crossing-only bench class for a check the construction
        // already guarantees.
        if let Some(chains) = self.crossing_only() {
            if chains.iter().any(|c| chain_degenerate(c)) {
                return None;
            }
            return Some(chains);
        }
        self.run_full()
    }

    /// An earlier form sorted every coordinate by x first, which was ~95 us of
    /// node_line's 796 us at n=4000.
    pub(super) fn revisit_dominated(&self) -> bool {
        let mut keys: Vec<u64> = self.coords.iter().map(|c| sortable_u64(c.x)).collect();
        let mut order: Vec<u32> = (0..keys.len() as u32).collect();
        radix_sort_keys_tls(&mut keys, &mut order);
        let mut max_freq = 1u32;
        let mut run = 1u32;
        for w in order.windows(2) {
            let a = self.coords[w[0] as usize];
            let b = self.coords[w[1] as usize];
            if a.x.to_bits() == b.x.to_bits() && a.y.to_bits() == b.y.to_bits() {
                run += 1;
                if run > max_freq {
                    max_freq = run;
                }
            } else {
                run = 1;
            }
        }
        max_freq * 2 + 2 >= self.n as u32
    }

    /// Fast path for the crossing-only class.
    pub(super) fn crossing_only(&mut self) -> Option<Vec<Vec<Coord<f64>>>> {
        if self.n < 2 {
            return None;
        }
        if self.full_span_dominated() {
            self.crossing_bail();
            return None;
        }
        self.adjacent_pass();
        if !self.nodes.is_empty() {
            self.crossing_bail();
            return None;
        }
        if self.eps_classes_anchors() {
            self.crossing_bail();
            return None;
        }
        let closed = self.closed();
        let limit = CROSSING_ONLY_PAIR_FACTOR * self.n + CROSSING_ONLY_PAIR_FLOOR;
        let mut st = ScanState {
            bail: false,
            calls: 0,
            limit,
        };
        let mut revisits: Vec<Coord<f64>> = Vec::new();
        #[cfg(feature = "arrange")]
        let indexed = self.enumerate_by_chain_index(&mut st, &mut revisits);
        #[cfg(not(feature = "arrange"))]
        let indexed = false;
        if !indexed {
            self.enumerate_segment_grid(&mut st, &mut revisits);
        }
        let bail = st.bail;
        if bail {
            self.crossing_bail();
            return None;
        }
        if self.eps_classes_present() {
            self.crossing_bail();
            return None;
        }
        for e in &self.nodes {
            self.splits[e.seg as usize].push(e.pt);
        }
        let mut breaks: Vec<(f64, f64)> = Vec::with_capacity(self.nodes.len() + revisits.len());
        for e in &self.nodes {
            breaks.push((e.pt.x, e.pt.y));
        }
        breaks.extend(revisits.iter().map(|c| (c.x, c.y)));
        breaks.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        breaks.dedup_by(|a, b| a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits());
        let mut starts: Vec<Coord<f64>> = Vec::with_capacity(self.n + self.nodes.len() + 1);
        // "simple by construction" claim (fuzz wkb seed 17790c39: DD-mixed node points
        // at raw^2 magnitude, dropped by these guards, release shipped)
        let mut unplaceable = false;
        for seg in 0..self.n {
            let a = self.a[seg];
            let b = self.b[seg];
            starts.push(a);
            if self.splits[seg].is_empty() {
                continue;
            }
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len = (dx * dx + dy * dy).sqrt();
            self.splits[seg].sort_by(|p, q| {
                let tp = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len;
                let tq = ((q.x - a.x) * dx + (q.y - a.y) * dy) / len;
                tp.partial_cmp(&tq).unwrap_or(core::cmp::Ordering::Equal)
            });
            let mut prev_dist = 0.0f64;
            let mut prev_pt: Option<Coord<f64>> = None;
            for &p in &self.splits[seg] {
                if let Some(q) = prev_pt
                    && q.x.to_bits() == p.x.to_bits()
                    && q.y.to_bits() == p.y.to_bits()
                {
                    continue;
                }
                prev_pt = Some(p);
                let d = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len;
                if d < -self.eps || d > len + self.eps {
                    unplaceable = true;
                    continue;
                }
                if (d - prev_dist).abs() <= self.eps || (len - d).abs() <= self.eps {
                    unplaceable = true;
                    continue;
                }
                starts.push(p);
                prev_dist = d;
            }
        }
        if unplaceable {
            self.crossing_bail();
            return None;
        }
        starts.push(self.coords[self.n]);
        let m = starts.len() - 1;
        let is_break = |k: usize| {
            breaks
                .binary_search_by(|e| {
                    e.0.total_cmp(&starts[k].x)
                        .then(e.1.total_cmp(&starts[k].y))
                })
                .is_ok()
        };
        // through degree-2 (non-break) boundaries
        let mut used = vec![false; m];
        let mut chains: Vec<Vec<Coord<f64>>> = Vec::new();
        for s in 0..m {
            if used[s] {
                continue;
            }
            used[s] = true;
            let mut chain: Vec<Coord<f64>> = vec![starts[s], starts[s + 1]];
            let mut cur = s;
            let mut cur_b = s;
            let front_key = (starts[s].x.to_bits(), starts[s].y.to_bits());
            loop {
                let next = if cur + 1 < m {
                    cur + 1
                } else if closed {
                    0
                } else {
                    break;
                };
                let boundary = if cur + 1 < m { cur + 1 } else { m };
                if used[next] || is_break(boundary) {
                    break;
                }
                let far = if next == 0 {
                    starts[1]
                } else {
                    starts[next + 1]
                };
                if (far.x.to_bits(), far.y.to_bits()) == front_key {
                    break;
                }
                used[next] = true;
                chain.push(far);
                cur = next;
            }
            let mut pre: Vec<Coord<f64>> = Vec::new();
            let back_key = (starts[cur + 1].x.to_bits(), starts[cur + 1].y.to_bits());
            loop {
                let prev = if cur_b > 0 {
                    cur_b - 1
                } else if closed {
                    m - 1
                } else {
                    break;
                };
                let boundary = if cur_b > 0 { cur_b } else { m };
                if used[prev] || is_break(boundary) {
                    break;
                }
                let far = starts[prev];
                if (far.x.to_bits(), far.y.to_bits()) == back_key {
                    break;
                }
                used[prev] = true;
                pre.push(far);
                cur_b = prev;
            }
            pre.reverse();
            pre.extend(chain);
            if pre.len() >= 2 {
                chains.push(pre);
            }
        }
        debug_assert!(
            chains
                .iter()
                .all(|c| !crate::validation::impls::check_linestring_self_intersection(c)),
            "crossing-only chains must be simple"
        );
        Some(chains)
    }

    /// On the x-scale bench, coordinates alternate 1e12 and 1e-12. The scan cost ~3.5
    /// ms and breached the CI gate by +62% (2026-09-22).
    fn full_span_dominated(&self) -> bool {
        const FULL_SPAN_FRACTION: f64 = 0.9;
        const FULL_SPAN_MIN_SEGMENTS: usize = 4;
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        for i in 0..self.n {
            min_x = min_x.min(self.lo_x[i]);
            max_x = max_x.max(self.hi_x[i]);
        }
        let bx = (max_x - min_x).abs().max(f64::MIN_POSITIVE);
        let mut count = 0usize;
        for i in 0..self.n {
            if (self.hi_x[i] - self.lo_x[i]) >= FULL_SPAN_FRACTION * bx {
                count += 1;
                if count >= FULL_SPAN_MIN_SEGMENTS {
                    return true;
                }
            }
        }
        false
    }

    /// Reset what the fast path touched, so the general path sees the noder as new()
    /// built it.
    fn enumerate_segment_grid(&mut self, st: &mut ScanState, revisits: &mut Vec<Coord<f64>>) {
        let mut boxes: Vec<[f64; 4]> = Vec::with_capacity(self.n);
        for i in 0..self.n {
            boxes.push([
                self.lo_x[i] - self.eps,
                self.hi_x[i] + self.eps,
                self.lo_y[i] - self.eps,
                self.hi_y[i] + self.eps,
            ]);
        }
        let mut grid = match SegGrid::build(&boxes) {
            Some(g) => g,
            None => {
                st.bail = true;
                return;
            }
        };
        for (j, bj) in boxes.iter().enumerate() {
            if st.bail {
                return;
            }
            grid.for_each_candidate(*bj, Some(j), |i| {
                if st.bail || i >= j {
                    return;
                }
                self.classify_pair(i, j, 0, 1, st, revisits);
            });
        }
    }

    /// Chain-index candidate enumeration.
    #[cfg(feature = "arrange")]
    fn enumerate_by_chain_index(
        &mut self,
        st: &mut ScanState,
        revisits: &mut Vec<Coord<f64>>,
    ) -> bool {
        // Disabled (CHAIN_INDEX_WORK_LIMIT == 0): return before building anything.
        // mono-chain construction and the chain-grid attempt on every call -measured
        // 2026-10-17 as a ~20 % regression on `ringing` against
        if CHAIN_INDEX_WORK_LIMIT == 0 {
            return false;
        }
        let lines: Vec<geo::Line<f64>> = (0..self.n)
            .map(|i| geo::Line::new(self.a[i], self.b[i]))
            .collect();
        let (mc, _) = crate::arrange::prep_intersect::build_mono_chains(&lines);
        let nc = mc.len();
        let mut chain_boxes: Vec<[f64; 4]> = Vec::with_capacity(nc);
        for c in &mc {
            chain_boxes.push([
                c.min_x - self.eps,
                c.max_x + self.eps,
                c.min_y - self.eps,
                c.max_y + self.eps,
            ]);
        }
        let mut grid = match SegGrid::build(&chain_boxes) {
            Some(g) => g,
            None => return false,
        };
        let limit = CHAIN_INDEX_WORK_LIMIT.saturating_mul(self.n);
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        let mut work: usize = 0;
        let mut over = false;
        for (ci, bi) in chain_boxes.iter().enumerate() {
            if over {
                break;
            }
            grid.for_each_candidate(*bi, Some(ci), |cj| {
                if over || cj <= ci {
                    return;
                }
                work += (mc[ci].end - mc[ci].start) + (mc[cj].end - mc[cj].start);
                if work > limit {
                    over = true;
                    return;
                }
                pairs.push((ci as u32, cj as u32));
            });
        }
        if over {
            return false;
        }
        for ci in 0..nc {
            self.enumerate_chain_pair(&mc, ci, ci, st, revisits);
            if st.bail {
                return true;
            }
        }
        for &(ci, cj) in &pairs {
            self.enumerate_chain_pair(&mc, ci as usize, cj as usize, st, revisits);
            if st.bail {
                return true;
            }
        }
        true
    }

    /// Enumerate every segment pair of two chains whose padded boxes can overlap.
    #[cfg(feature = "arrange")]
    fn enumerate_chain_pair(
        &mut self,
        mc: &[crate::arrange::prep_intersect::MonoChain],
        ia: usize,
        ib: usize,
        st: &mut ScanState,
        revisits: &mut Vec<Coord<f64>>,
    ) {
        if st.bail {
            return;
        }
        let (a, b) = (&mc[ia], &mc[ib]);
        let (as_, ae) = (a.start, a.end);
        let (bs, be) = (b.start, b.end);
        if as_ >= ae || bs >= be {
            return;
        }
        let use_y = (a.max_x - a.min_x).abs().min((b.max_x - b.min_x).abs())
            < (a.max_y - a.min_y).abs().min((b.max_y - b.min_y).abs());
        let a_asc = if use_y {
            a.quad == 0 || a.quad == 3
        } else {
            a.quad <= 1
        };
        let b_asc = if use_y {
            b.quad == 0 || b.quad == 3
        } else {
            b.quad <= 1
        };
        let bn = be - bs;
        let bidx = |p: usize| if b_asc { bs + p } else { be - 1 - p };
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        {
            let (lo, hi): (&[f64], &[f64]) = if use_y {
                (&self.lo_y, &self.hi_y)
            } else {
                (&self.lo_x, &self.hi_x)
            };
            let (mut p_lo, mut p_hi) = (0usize, 0usize);
            for step in 0..(ae - as_) {
                let i = if a_asc { as_ + step } else { ae - 1 - step };
                let i0 = lo[i];
                let i1 = hi[i];
                while p_lo < bn && hi[bidx(p_lo)] < i0 {
                    p_lo += 1;
                }
                if p_hi < p_lo {
                    p_hi = p_lo;
                }
                while p_hi < bn && lo[bidx(p_hi)] <= i1 {
                    p_hi += 1;
                }
                for p in p_lo..p_hi {
                    pairs.push((i as u32, bidx(p) as u32));
                }
            }
        }
        for &(i, j) in &pairs {
            self.classify_pair(i as usize, j as usize, ia, ib, st, revisits);
            if st.bail {
                return;
            }
        }
    }

    /// Leaf of `descend`: the per-pair gates and the classification the old segment
    /// scan applied, verbatim, so the pair set and the `calls` budget are unchanged.
    fn classify_pair(
        &mut self,
        i: usize,
        j: usize,
        ia: usize,
        ib: usize,
        st: &mut ScanState,
        revisits: &mut Vec<Coord<f64>>,
    ) {
        if i == j || (ia == ib && j <= i) {
            return;
        }
        let bi = [
            self.lo_x[i] - self.eps,
            self.hi_x[i] + self.eps,
            self.lo_y[i] - self.eps,
            self.hi_y[i] + self.eps,
        ];
        let bj = [
            self.lo_x[j] - self.eps,
            self.hi_x[j] + self.eps,
            self.lo_y[j] - self.eps,
            self.hi_y[j] + self.eps,
        ];
        if bi[1] < bj[0] || bj[1] < bi[0] || bi[3] < bj[2] || bj[3] < bi[2] {
            return;
        }
        if j == i + 1
            || i == j + 1
            || (self.closed() && ((i == 0 && j == self.n - 1) || (j == 0 && i == self.n - 1)))
        {
            return;
        }
        st.calls += 1;
        if st.calls > st.limit {
            st.bail = true;
            return;
        }
        match classify::classify(self.a[i], self.b[i], self.a[j], self.b[j], self.eps) {
            Hit::None => {}
            Hit::Shared => {
                let v = if self.a[i] == self.a[j] || self.a[i] == self.b[j] {
                    self.a[i]
                } else {
                    self.b[i]
                };
                revisits.push(v);
            }
            Hit::Cross(pt) => {
                self.nodes.push(NodeEnt { seg: i as u32, pt });
                self.nodes.push(NodeEnt { seg: j as u32, pt });
            }
            Hit::Collinear => {
                st.bail = true;
            }
            Hit::VertexOnEdge(v) => {
                let seg = if v == self.a[i] || v == self.b[i] {
                    j
                } else {
                    i
                };
                self.nodes.push(NodeEnt {
                    seg: seg as u32,
                    pt: v,
                });
            }
        }
    }

    fn crossing_bail(&mut self) {
        for e in &self.nodes {
            self.splits[e.seg as usize].clear();
        }
        self.nodes.clear();
    }

    /// True when two distinct points among the input anchors and the node points lie
    /// within eps (Chebyshev) of each other.
    fn eps_classes_present(&self) -> bool {
        self.eps_classes_scan(&self.nodes)
    }

    /// Anchor-only eps classes, checked before the candidate scan: an anchor-level
    /// near-coincidence already forces the general path, so the scan (and its classify
    fn eps_classes_anchors(&self) -> bool {
        self.eps_classes_scan(&[])
    }

    /// An earlier sort cost ~170 us of 796 us at n=4000.
    fn eps_classes_scan(&self, nodes: &[NodeEnt]) -> bool {
        let na = self.coords.len();
        let total = na + nodes.len();
        let mut pts: Vec<(f64, f64)> = Vec::with_capacity(total);
        for c in self.coords {
            pts.push((c.x, c.y));
        }
        for e in nodes {
            pts.push((e.pt.x, e.pt.y));
        }
        let mut keys: Vec<u64> = Vec::with_capacity(total);
        let mut order: Vec<u32> = Vec::with_capacity(total);
        for (i, p) in pts.iter().enumerate() {
            keys.push(sortable_u64(p.0));
            order.push(i as u32);
        }
        radix_sort_keys_tls(&mut keys, &mut order);
        // (the two-array index indirection measured 73 us on the 1000v figure-8,
        // 2026-09-18).
        let mut sorted: Vec<(f64, f64)> = order.iter().map(|&o| pts[o as usize]).collect();
        let budget = EPS_CLASS_SCAN_FACTOR * total + EPS_CLASS_SCAN_FLOOR;
        let mut iters = 0usize;
        let mut i = 0usize;
        while i < total {
            let xi = sorted[i].0;
            let mut run_end = i + 1;
            while run_end < total && sorted[run_end].0.to_bits() == xi.to_bits() {
                run_end += 1;
            }
            if run_end - i > 1 {
                sorted[i..run_end].sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
            }
            for j in i..run_end {
                let (xi, yi) = sorted[j];
                for &(xj, yj) in &sorted[j + 1..run_end] {
                    if yj - yi > self.eps {
                        break;
                    }
                    if xj.to_bits() == xi.to_bits() && yj.to_bits() == yi.to_bits() {
                        continue;
                    }
                    iters += 1;
                    if iters > budget {
                        return true;
                    }
                    if (yj - yi).abs() <= self.eps {
                        return true;
                    }
                }
                for &(xj, yj) in &sorted[run_end..] {
                    if xj - xi > self.eps {
                        break;
                    }
                    iters += 1;
                    if iters > budget {
                        return true;
                    }
                    if (yj - yi).abs() <= self.eps {
                        return true;
                    }
                }
            }
            i = run_end;
        }
        false
    }

    /// The general path: exact-collinear families, adjacency noding, the 2-D sweep, eps
    /// clustering, piece construction and reconnection.
    pub(super) fn run_full(&mut self) -> Option<Vec<Vec<Coord<f64>>>> {
        self.family_pass();
        let pure_collinear = if self.n > 0 && self.family[0] != NO_FAMILY {
            self.family_size[self.family[0] as usize] == self.n
        } else {
            false
        };
        if pure_collinear {
            for e in &self.nodes {
                self.splits[e.seg as usize].push(e.pt);
            }
        } else {
            self.adjacent_pass();
            self.sweep_pass()?;
            self.cluster_and_split();
        }
        let pieces = if pure_collinear {
            self.build_pieces_direct()
        } else {
            self.build_pieces()
        };
        let mut pieces = self.dedup_pieces(pieces);
        if self.near_collinear {
            self.dedup_pieces_tolerant(&mut pieces);
        }
        let chains = self.reconnect(pieces);
        for chain in &chains {
            // (n < 2 early-out) but fail the validator (RepeatedPoint /
            // RingTooFewPoints) - a computed [P, P] chain shipped once (fuzz 55853: eps
            // clustering collapsed both split points of a piece onto one coordinate).
            if chain_degenerate(chain)
                || crate::validation::impls::check_linestring_self_intersection(chain)
            {
                return None;
            }
        }
        if chains.is_empty() {
            if chain_degenerate(self.coords)
                || crate::validation::impls::check_linestring_self_intersection(self.coords)
            {
                return None;
            }
            return Some(vec![self.coords.to_vec()]);
        }
        Some(chains)
    }


    fn family_pass(&mut self) {
        let mut buckets: FxHashMap<(u64, u64), Vec<u32>> = FxHashMap::default();
        for i in 0..self.n {
            let dx = self.b[i].x - self.a[i].x;
            let dy = self.b[i].y - self.a[i].y;
            let m = dx.abs().max(dy.abs());
            if m == 0.0 {
                continue;
            }
            let (mut nx, mut ny) = (dx / m, dy / m);
            if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
                nx = -nx;
                ny = -ny;
            }
            // The negation turns +0.0 into -0.0.
            if ny == 0.0 {
                ny = 0.0;
            }
            if nx == 0.0 {
                nx = 0.0;
            }
            buckets
                .entry((nx.to_bits(), ny.to_bits()))
                .or_default()
                .push(i as u32);
        }
        let mut fam: u32 = 0;
        for (_, members) in buckets {
            if members.len() < 2 {
                continue;
            }
            let mut os: Vec<(f64, f64, u32)> = Vec::with_capacity(members.len());
            for &i in &members {
                let i = i as usize;
                let dx = self.b[i].x - self.a[i].x;
                let dy = self.b[i].y - self.a[i].y;
                let m = dx.abs().max(dy.abs());
                let (mut nx, mut ny) = (dx / m, dy / m);
                if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
                    nx = -nx;
                    ny = -ny;
                }
                let mag = (nx * self.a[i].y).abs() + (ny * self.a[i].x).abs();
                let mut o = nx * self.a[i].y - ny * self.a[i].x;
                if o == 0.0 {
                    o = 0.0;
                }
                os.push((o, mag, i as u32));
            }
            os.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(core::cmp::Ordering::Equal));
            let mut groups: Vec<Vec<u32>> = Vec::new();
            let mut cur: Vec<u32> = vec![os[0].2];
            let mut cur_o = os[0].0;
            let mut cur_mag = os[0].1;
            for &(o, mag, i) in &os[1..] {
                // ~32 ulps of the offset terms
                let tol = 32.0 * f64::EPSILON * (cur_mag + mag);
                if (o - cur_o).abs() <= tol {
                    cur.push(i);
                } else {
                    groups.push(core::mem::take(&mut cur));
                    cur.push(i);
                    cur_o = o;
                }
                cur_mag = mag;
            }
            groups.push(cur);
            for g in groups {
                if g.len() < 2 {
                    continue;
                }
                let id = fam;
                fam += 1;
                for &i in &g {
                    self.family[i as usize] = id;
                }
                self.family_size.push(g.len());
                self.node_family_1d(&g);
            }
        }
    }

    /// 1-D interval noding for an exact-collinear family: every member splits at every
    /// other member's original endpoints that fall strictly inside its span (beyond eps
    fn node_family_1d(&mut self, members: &[u32]) {
        let r = members[0] as usize;
        let dx = self.b[r].x - self.a[r].x;
        let dy = self.b[r].y - self.a[r].y;
        let len2 = dx * dx + dy * dy;
        if len2 == 0.0 {
            return;
        }
        let t_of = |p: Coord<f64>| ((p.x - self.a[r].x) * dx + (p.y - self.a[r].y) * dy) / len2;
        let mut endpts: Vec<(f64, Coord<f64>, u32)> = Vec::with_capacity(members.len() * 2);
        for &m in members {
            let m = m as usize;
            endpts.push((t_of(self.a[m]), self.a[m], m as u32));
            endpts.push((t_of(self.b[m]), self.b[m], m as u32));
        }
        endpts.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(core::cmp::Ordering::Equal));
        let inv_len = len2.sqrt().recip();
        let eps_t = self.eps * inv_len;
        for &m in members {
            let m = m as usize;
            let (t0, t1) = (t_of(self.a[m]), t_of(self.b[m]));
            let (t_lo, t_hi) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
            let lo_i = endpts.partition_point(|e| e.0 <= t_lo + eps_t);
            let hi_i = endpts.partition_point(|e| e.0 < t_hi - eps_t);
            // span collapses inside the eps merge band (t_hi - t_lo <= 2*eps_t,
            // including eps_t = inf when len2 underflows)
            if lo_i >= hi_i {
                continue;
            }
            for e in &endpts[lo_i..hi_i] {
                if e.2 == m as u32 {
                    continue;
                }
                self.nodes.push(NodeEnt {
                    seg: m as u32,
                    pt: e.1,
                });
            }
        }
    }


    fn adjacent_pass(&mut self) {
        let eps = self.eps;
        for i in 0..self.n - 1 {
            if segments_collinear_overlap(
                self.coords[i],
                self.coords[i + 1],
                self.coords[i + 1],
                self.coords[i + 2],
                eps,
            ) {
                self.nodes.push(NodeEnt {
                    seg: i as u32,
                    pt: self.coords[i + 2],
                });
                self.nodes.push(NodeEnt {
                    seg: (i + 1) as u32,
                    pt: self.coords[i],
                });
            }
        }
        if self.closed()
            && segments_collinear_overlap(
                self.coords[self.n - 1],
                self.coords[self.n],
                self.coords[0],
                self.coords[1],
                eps,
            )
        {
            self.nodes.push(NodeEnt {
                seg: (self.n - 1) as u32,
                pt: self.coords[1],
            });
            self.nodes.push(NodeEnt {
                seg: 0,
                pt: self.coords[self.n - 1],
            });
        }
    }


    fn sweep_pass(&mut self) -> Option<()> {
        let mut sweep_ids: Vec<u32> = Vec::new();
        for i in 0..self.n {
            let f = self.family[i];
            if f == NO_FAMILY || self.family_size[f as usize] <= FAMILY_IN_SWEEP_LIMIT {
                sweep_ids.push(i as u32);
            }
        }
        if sweep_ids.len() >= 2 {
            let mut keys: Vec<u64> = sweep_ids
                .iter()
                .map(|&i| sortable_u64(self.lo_x[i as usize]))
                .collect();
            let mut order: Vec<u32> = sweep_ids.clone();
            radix_sort_keys_tls(&mut keys, &mut order);
            let mut active: Vec<u32> = Vec::new();
            for &ord in &order {
                let j = ord as usize;
                if active.len() > SWEEP_ACTIVE_LIMIT {
                    return None;
                }
                active.retain(|&p| self.hi_x[p as usize] + self.eps >= self.lo_x[j]);
                for &p in &active {
                    let i = p as usize;
                    if self.hi_y[i] < self.lo_y[j] - self.eps
                        || self.lo_y[i] > self.hi_y[j] + self.eps
                    {
                        continue;
                    }
                    if self.family[i] != NO_FAMILY && self.family[i] == self.family[j] {
                        continue;
                    }
                    self.test_pair(i, j);
                }
                active.push(ord);
            }
        }
        let mut by_family: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
        for i in 0..self.n {
            let f = self.family[i];
            if f != NO_FAMILY && self.family_size[f as usize] > FAMILY_IN_SWEEP_LIMIT {
                by_family.entry(f).or_default().push(i as u32);
            }
        }
        if !by_family.is_empty() {
            self.family_members = by_family;
            // of ~n/2 (measured 3.05 ms of a 6.19 ms 4000-vertex repair. Half of the
            // noder's time once the other)
            let mut sweep_boxes: Vec<[f64; 4]> = Vec::with_capacity(sweep_ids.len());
            for &s in &sweep_ids {
                let s = s as usize;
                sweep_boxes.push([
                    self.lo_x[s] - self.eps,
                    self.hi_x[s] + self.eps,
                    self.lo_y[s] - self.eps,
                    self.hi_y[s] + self.eps,
                ]);
            }
            let mut sweep_grid = SegGrid::build(&sweep_boxes);
            let removed_lists: Vec<Vec<u32>> = self.family_members.values().cloned().collect();
            let mut hits: Vec<u32> = Vec::new();
            for members in &removed_lists {
                for &i in members {
                    let i = i as usize;
                    let bi = [
                        self.lo_x[i] - self.eps,
                        self.hi_x[i] + self.eps,
                        self.lo_y[i] - self.eps,
                        self.hi_y[i] + self.eps,
                    ];
                    match sweep_grid.as_mut() {
                        Some(g) => {
                            hits.clear();
                            g.for_each_candidate(bi, None, |pos| {
                                hits.push(sweep_ids[pos]);
                            });
                            for &s in &hits {
                                let s = s as usize;
                                if self.bbox_gate(i, s) {
                                    self.test_pair(i, s);
                                }
                            }
                        }
                        None => {
                            for &s in &sweep_ids {
                                let s = s as usize;
                                if self.bbox_gate(i, s) {
                                    self.test_pair(i, s);
                                }
                            }
                        }
                    }
                }
            }
            let fams: Vec<u32> = self.family_members.keys().copied().collect();
            for (k, &f1) in fams.iter().enumerate() {
                for &f2 in &fams[k + 1..] {
                    self.test_family_pair(f1, f2);
                }
            }
        }
        Some(())
    }

    /// Parallel lines below FAMILY_PARALLEL_SIN (family_parallel_sin). Apart by more
    /// than FAMILY_PARALLEL_SKIP_DIST (family_parallel_skip_dist) x scale, give zero
    /// pair tests. The 64 eps x len and 1e-12 x len validator flags stay inside that
    /// gap. Lines at or above FAMILY_SHALLOW_SIN (family_shallow_sin) meet at one point
    /// P. Only spans that hold P, plus FAMILY_SPAN_PAD_EPS (family_span_pad_eps) x eps,
    /// get tested. Drift is 32 ulp over sin. Angles below ~1e-3 keep the full scan:
    /// drift grows as 1/sin.
    fn test_family_pair(&mut self, f1: u32, f2: u32) {
        let m1: Vec<u32> = self.family_members[&f1].clone();
        let m2: Vec<u32> = self.family_members[&f2].clone();
        let r1 = m1[0] as usize;
        let r2 = m2[0] as usize;
        let d1x = self.b[r1].x - self.a[r1].x;
        let d1y = self.b[r1].y - self.a[r1].y;
        let d2x = self.b[r2].x - self.a[r2].x;
        let d2y = self.b[r2].y - self.a[r2].y;
        let l1 = (d1x * d1x + d1y * d1y).sqrt();
        let l2 = (d2x * d2x + d2y * d2y).sqrt();
        if l1 == 0.0 || l2 == 0.0 {
            for &i in &m1 {
                for &j in &m2 {
                    if self.bbox_gate(i as usize, j as usize) {
                        self.test_pair(i as usize, j as usize);
                    }
                }
            }
            return;
        }
        let cross = d1x * d2y - d1y * d2x;
        let sin_abs = cross.abs() / (l1 * l2);
        if sin_abs < FAMILY_SHALLOW_SIN {
            if sin_abs < FAMILY_PARALLEL_SIN {
                let wx = self.a[r2].x - self.a[r1].x;
                let wy = self.a[r2].y - self.a[r1].y;
                let dist = (wx * d1y - wy * d1x).abs() / l1;
                if dist > FAMILY_PARALLEL_SKIP_DIST * self.scale {
                    return;
                }
            }
            for &i in &m1 {
                for &j in &m2 {
                    if self.bbox_gate(i as usize, j as usize) {
                        self.test_pair(i as usize, j as usize);
                    }
                }
            }
            return;
        }
        let t = ((self.a[r2].x - self.a[r1].x) * d2y - (self.a[r2].y - self.a[r1].y) * d2x) / cross;
        let px = self.a[r1].x + t * d1x;
        let py = self.a[r1].y + t * d1y;
        let pad = FAMILY_SPAN_PAD_EPS * self.eps;
        let tp1 = ((px - self.a[r1].x) * d1x + (py - self.a[r1].y) * d1y) / (l1 * l1);
        let tp2 = ((px - self.a[r2].x) * d2x + (py - self.a[r2].y) * d2y) / (l2 * l2);
        let mut cand1: SmallVec<[u32; 16]> = SmallVec::new();
        let mut cand2: SmallVec<[u32; 16]> = SmallVec::new();
        for &i in &m1 {
            let i = i as usize;
            let s0 = ((self.a[i].x - self.a[r1].x) * d1x + (self.a[i].y - self.a[r1].y) * d1y)
                / (l1 * l1);
            let s1 = ((self.b[i].x - self.a[r1].x) * d1x + (self.b[i].y - self.a[r1].y) * d1y)
                / (l1 * l1);
            let (lo, hi) = if s0 < s1 { (s0, s1) } else { (s1, s0) };
            let tpad = pad / l1;
            if lo - tpad <= tp1 && tp1 <= hi + tpad {
                cand1.push(i as u32);
            }
        }
        for &j in &m2 {
            let j = j as usize;
            let s0 = ((self.a[j].x - self.a[r2].x) * d2x + (self.a[j].y - self.a[r2].y) * d2y)
                / (l2 * l2);
            let s1 = ((self.b[j].x - self.a[r2].x) * d2x + (self.b[j].y - self.a[r2].y) * d2y)
                / (l2 * l2);
            let (lo, hi) = if s0 < s1 { (s0, s1) } else { (s1, s0) };
            let tpad = pad / l2;
            if lo - tpad <= tp2 && tp2 <= hi + tpad {
                cand2.push(j as u32);
            }
        }
        for &i in &cand1 {
            for &j in &cand2 {
                if self.bbox_gate(i as usize, j as usize) {
                    self.test_pair(i as usize, j as usize);
                }
            }
        }
    }

    /// eps-padded bbox gate, identical to `edges_intersect_general`'s.
    #[inline]
    fn bbox_gate(&self, i: usize, j: usize) -> bool {
        let ai = self.a[i];
        let bi = self.b[i];
        let aj = self.a[j];
        let bj = self.b[j];
        if ai == aj || ai == bj || bi == aj || bi == bj {
            return false;
        }
        !(self.hi_x[i] < self.lo_x[j] - self.eps
            || self.lo_x[i] > self.hi_x[j] + self.eps
            || self.hi_y[i] < self.lo_y[j] - self.eps
            || self.lo_y[i] > self.hi_y[j] + self.eps)
    }
}

#[cfg(test)]
#[path = "line_tests.rs"]
mod tests;

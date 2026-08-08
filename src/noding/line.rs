//! Lean line-specific noder: repair a non-simple LineString by SPLITTING at
//! every self-intersection (noding) instead of dropping conflicting segments.
//!
//! The old line repair (the `simple_subline` greedy filter in make_valid)
//! walks the segments and DROPS any segment that conflicts with an
//! already-kept one. It is O(n·kept) with a bbox prefilter - measured
//! 1892 µs on the 5000-vertex lissajous - and it loses data (the dropped
//! traversal is gone). This module replaces it with a noder: segments are
//! split at their intersection points and the pieces are reconnected into
//! simple chains. Splitting preserves the full traversal.
//!
//! Design contract (every choice below mirrors the validator's own
//! predicates so the noded output passes `check_linestring_self_intersection`
//! per chain and the cross-component rule of the MultiLineString validator):
//!
//! 1. Detection uses the validator's tolerance semantics exactly:
//!    - eps = 1e-12 * bbox scale for the collinear t-overlap gate,
//!    - the adaptive 32-ulp per-orient product-sum bound for the
//!      collinear DETECTION gate (same as `edges_intersect_general`),
//!    - the segment-local `1e-12 * len²` tolerance for vertex-on-edge
//!      touches (same as `point_strictly_on_segment`).
//! 2. Node points: proper crossings get the DD (double-double) intersection
//!    point, shared by both segments; collinear overlaps split at the OTHER
//!    segment's original endpoints; vertex-on-edge splits at the offending
//!    vertex. All collinear nodes are original vertices (bit-exact).
//! 3. Node points are CLUSTERED at the validator's own eps (1e-12 * scale)
//!    and every segment with a node in a cluster splits at the SAME
//!    canonical point, preferring an original vertex anchor. This is what
//!    kills the eps-class the earlier noding experiment hit: sub-tolerance
//!    topology is snapped away instead of being flagged by the validator's
//!    coarser tests. The canonical point may sit up to ~2 eps off a
//!    segment's line; the resulting kink is accepted by the adjacent-pair
//!    tests (the collinear gate is `o2.abs() > eps`, and the kink's orient
//!    is L·δ with δ ≤ 2 eps, so it rejects as "not collinear" rather than
//!    flagging an overlap), and the shared endpoint makes any residual
//!    micro-crossing invisible to the strict zero-safe predicates.
//! 4. Exact-collinear families are detected up front (normalized-direction
//!    bucket, offset sort, tolerant merge) and noded in 1-D at the other
//!    members' endpoints. Families with more than 64 members are removed
//!    from the 2-D sweep so the active set cannot explode on dense
//!    collinear inputs (the `collinear ov` bench case); their cross-family
//!    pairs are tested directly.
//! 5. Pieces are deduplicated (exact canonical endpoint key; plus a
//!    tolerant validator-predicate sweep when near-collinear noding
//!    happened, which drops coincident/overlapping residuals).
//! 6. Reconnection chains only through degree-2 vertices, so a node or
//!    revisit vertex (degree >= 3) is always a chain boundary and
//!    cross-component contact is boundary-only.
//! 7. Every output chain is re-validated with the validator's own
//!    predicate; `None` signals the caller to fall back to the greedy
//!    filter. The noder is conservative by construction.

use alloc::vec;
use alloc::vec::Vec;

use geo::Coord;

use rustc_hash::FxHashMap;

use crate::validation::impls::segments_collinear_overlap;
use crate::validation::sweep::{radix_sort_keys_tls, sortable_u64};

mod classify;
mod pieces;
const NO_FAMILY: u32 = u32::MAX;
const NO_SEG: u32 = u32::MAX;
/// Active-set limit for the 2-D sweep; beyond it the noder bails to the
/// caller's fallback (a dense active set means the retain loop is O(n²)
/// anyway - the family pass already handles the dense collinear class).
const SWEEP_ACTIVE_LIMIT: usize = 512;
/// Families larger than this are removed from the 2-D sweep (their members
/// span long x-ranges and would pin the active set open); cross-family
/// pairs are tested directly instead.
const FAMILY_IN_SWEEP_LIMIT: usize = 64;

/// Outcome of the lean per-pair test (mirrors the validator's predicate
/// chain: fast-FP first, robust escalation, collinear, vertex-on-edge,
/// shared endpoint).
#[derive(Clone, Copy, Debug)]
enum Hit {
    /// No intersection per the validator's predicates.
    None,
    /// Segments share an endpoint exactly (vertex revisit). No noding is
    /// needed - the shared vertex is already common topology; the chain
    /// reconnection breaks there via the degree rule.
    Shared,
    /// Proper crossing; the point is the DD intersection, shared by both
    /// segments.
    Cross(Coord<f64>),
    /// Collinear overlap (adaptive gate, t-overlap beyond eps). Nodes are
    /// the OTHER segment's original endpoints.
    Collinear,
    /// The returned vertex lies strictly on the other segment's interior
    /// (segment-local tolerance). Node = that vertex.
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
    /// Set when a near-collinear (non-exact) overlap pair was noded; the
    /// tolerant piece-dedup sweep then runs to drop coincident residuals.
    near_collinear: bool,
    splits: Vec<Vec<Coord<f64>>>,
    /// Cluster canonical lookup for piece-endpoint snapping.
    canon_map: FxHashMap<(u64, u64), Coord<f64>>,
}

/// Node a non-simple line into simple chains. Returns `None` when the
/// noder cannot guarantee a valid result (pathological density, or a chain
/// that fails the validator) - the caller falls back to the greedy filter.
pub(crate) fn node_line(coords: &[Coord<f64>]) -> Option<Vec<Vec<Coord<f64>>>> {
    let n = coords.len() - 1;
    if n < 2 {
        return Some(vec![coords.to_vec()]);
    }
    let mut ln = LineNoder::new(coords);
    ln.run()
}

impl<'a> LineNoder<'a> {
    fn new(coords: &'a [Coord<f64>]) -> Self {
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
        }
    }

    fn closed(&self) -> bool {
        self.coords[0] == self.coords[self.n]
    }

    fn run(&mut self) -> Option<Vec<Vec<Coord<f64>>>> {
        // Revisit-dominated inputs (e.g. spoke wheels: half the coords are
        // one shared vertex) have no crossings to node - the 2-D sweep
        // would pay O(F^2) Shared classifications for nothing. The greedy
        // filter resolves the revisits in O(n); fall back to it. Small
        // inputs keep the full noder (the sweep is cheap there and the
        // crossing cases need it).
        if self.n >= 32 {
            // Radix the coords by x-bits and count equal (x, y) runs - the
            // spoke wheel's shared vertex shows up as one long run, O(n)
            // with a much smaller constant than a hashmap.
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
            if max_freq * 2 + 2 >= self.n as u32 {
                return None;
            }
        }
        self.family_pass();
        // Pure-collinear fast path: every segment lies on one exact line.
        // The 1-D family noding is then the complete noding - the 2-D
        // sweep/cluster machinery adds nothing. Split points are the other
        // members' original endpoints, which are already bit-exact anchors.
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
            if crate::validation::impls::check_linestring_self_intersection(chain) {
                return None;
            }
        }
        if chains.is_empty() {
            return Some(vec![self.coords.to_vec()]);
        }
        Some(chains)
    }

    // ---------------------------------------------------------------------
    // Phase 1: exact-collinear families (1-D noding)
    // ---------------------------------------------------------------------

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
            // The negation turns +0.0 into -0.0, which has different bits
            // and would split one exact direction into two buckets.
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
            // Offset (cross of the normalized direction with the anchor) +
            // term magnitude for the merge tolerance.
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
                // Merge tolerance: ~32 ulps of the offset terms (the o value
                // for one exact line spreads by a few ulps across anchors;
                // real line separations above ~100 ulps stay separate - and
                // anything within the 32-ulp band is inside the validator's
                // collinear tolerance anyway, so merging is semantically
                // consistent with the validator).
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

    /// 1-D interval noding for an exact-collinear family: every member
    /// splits at every OTHER member's original endpoints that fall strictly
    /// inside its span (beyond eps of its own endpoints).
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
            for e in &endpts[lo_i..hi_i] {
                // Strict interior: the member's own endpoints sit at the
                // range boundaries and are excluded; another member's
                // endpoint coinciding with this member's endpoint is too.
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

    // ---------------------------------------------------------------------
    // Phase 2: adjacent + closure collinear-overlap pairs
    // ---------------------------------------------------------------------

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

    // ---------------------------------------------------------------------
    // Phase 3: 2-D sweep over all remaining pairs
    // ---------------------------------------------------------------------

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
        // Removed members (large families) vs sweep members and vs members
        // of other large families. Same-family pairs are already noded by
        // the 1-D pass, so removed members are grouped by family and only
        // cross-family pairs are tested - an all-collinear input must not
        // pay an O(F^2) same-family scan.
        let mut removed: Vec<u32> = Vec::new();
        for i in 0..self.n {
            let f = self.family[i];
            if f != NO_FAMILY && self.family_size[f as usize] > FAMILY_IN_SWEEP_LIMIT {
                removed.push(i as u32);
            }
        }
        if !removed.is_empty() {
            let mut by_family: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
            for &i in &removed {
                by_family
                    .entry(self.family[i as usize])
                    .or_default()
                    .push(i);
            }
            // Large-family members vs sweep members.
            for (_, members) in by_family.iter() {
                for &i in members {
                    let i = i as usize;
                    for &s in &sweep_ids {
                        let s = s as usize;
                        if self.bbox_gate(i, s) {
                            self.test_pair(i, s);
                        }
                    }
                }
            }
            // Cross-family removed-vs-removed pairs.
            let fams: Vec<u32> = by_family.keys().copied().collect();
            for (k, &f1) in fams.iter().enumerate() {
                let m1 = &by_family[&f1];
                for &f2 in &fams[k + 1..] {
                    let m2 = &by_family[&f2];
                    for &i in m1 {
                        let i = i as usize;
                        for &j in m2 {
                            let j = j as usize;
                            if self.bbox_gate(i, j) {
                                self.test_pair(i, j);
                            }
                        }
                    }
                }
            }
        }
        Some(())
    }

    /// eps-padded bbox gate, identical to `edges_intersect_general`'s.
    /// Pairs sharing an endpoint classify as Shared anyway (the revisit is
    /// resolved by the dedup + reconnect, not by a split), so they are
    /// skipped here - spoke-wheel inputs must not pay the O(F^2) classify.
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

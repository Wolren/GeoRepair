use alloc::vec::Vec;
use geo::Line;
use rstar::{AABB, RTree, RTreeObject};

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
    // Zero-safe strict opposite sign. The (o1 > 0) != (o2 > 0) form treats
    // an EXACT zero orient (a collinear touch — e.g. a snapped vertex
    // landing on another edge's line) as a crossing when the paired orient
    // is positive, flagging GEOS-valid structure output. Measured: 276/300
    // real-world repaired components rejected by the --fast gate while the
    // full validator and GEOS accept them (2026-08-03). Matches
    // edges_intersect_general's proper-crossing semantics.
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

pub(crate) struct MonoChain {
    start: usize,
    end: usize,
    quad: u8,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    ring_id: u32,
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
fn build_mono_chains(lines: &[Line<f64>]) -> (Vec<MonoChain>, (f64, f64, f64, f64)) {
    let n = lines.len();
    if n == 0 {
        return (vec![], (f64::MAX, f64::MIN, f64::MAX, f64::MIN));
    }

    // Ring boundaries are detected inline (segment start != previous
    // segment end) - no separate pass. The per-ring scale-relative eps is
    // likewise computed from a running ring bbox at the ring's end and
    // stamped on its chains (measured 2026-08-07: the old pre-scan cost
    // ~15 us on a 5000-vertex ring). The GLOBAL bbox over all chains is
    // accumulated here too, so the grid path does not re-scan the chains
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
        // Force chain break at ring boundary
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
        // Closing pair is NOT skipped: shared vertex 0 aside, the two edges
        // can overlap collinearly (backtracking closure), a genuine
        // self-intersection; matches check_ring_validity.
        if mc1.ring_id == mc2.ring_id && (j == i + 1 || j + 1 == i) {
            return false;
        }
        let li = &lines[i];
        let lj = &lines[j];
        if segments_properly_cross(li, lj) {
            return true;
        }
        // Same-ring pairs use the validator's FULL pair predicate (proper
        // crossing + eps-collinear overlap + vertex-on-edge T-junction) with
        // the RING's own scale-relative eps - the gate must accept exactly
        // what check_ring_validity accepts, or the Fast path could ship a
        // polygon the exit validator would reject (2026-08-07). Cross-ring
        // pairs stay on proper crossings only (hole vertex on shell edge is
        // a VALID OGC touch; collinear hole-shell overlaps are caught by the
        // hole checks in the gate).
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
    // Assign ring ids: a segment whose start != previous segment's end
    // starts a new ring (same rule as build_mono_chains). Track each
    // ring's bbox to derive its scale-relative eps (same formula as
    // check_ring_validity - the gate must not use a poly-global eps,
    // which would loosen micro-ring tolerances inside big shells).
    let mut ring_of = [0u32; crate::core::SMALL_RING_LINES + 1];
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
    let mut eps_by_ring = vec![0.0f64; nrings as usize];
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
                // Same ring: skip adjacent edges. The closing pair (first
                // vs last edge) is NOT skipped — the two share vertex 0 but
                // can overlap collinearly beyond it (backtracking closure),
                // a genuine self-intersection; matches check_ring_validity.
                if j == i + 1 || j + 1 == i {
                    continue;
                }
            }
            let lj = &lines[j];
            if segments_properly_cross(li, lj) {
                return false;
            }
            // Same-ring pairs use the validator's FULL pair predicate
            // (proper crossing + eps-collinear overlap + vertex-on-edge
            // T-junction) with the ring's own eps - identical to
            // check_ring_validity's sweep (2026-08-07). Cross-ring pairs
            // stay on proper crossings only (hole vertex on shell edge is
            // a VALID OGC touch; collinear hole-shell overlaps are caught
            // by the hole checks in the gate).
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
    has_no_intersections_nan_ok(lines)
}

/// `has_no_intersections` for line arrays whose finiteness was already
/// verified by the caller (the fast-path gate's `ring_is_plausible` scan
/// checks every coordinate before collecting the lines). Saves one full
/// pass over the lines on the valid-polygon path (measured 2026-08-09:
/// the NaN scan is a redundant ~1-2 us on a 5000-vertex ring - and one
/// less pass of memory traffic on the bandwidth-bound parallel rows).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn has_no_intersections_nan_ok(lines: &[Line<f64>]) -> bool {
    let n = lines.len();
    if n == 0 {
        return true;
    }

    // Small-ring fast path: direct O(n²) pairwise sweep with no allocations.
    // The monotone-chain + grid + R-tree machinery below allocates multiple
    // Vecs and only pays off for large inputs (measured: 95.6% of the
    // 1.58M real-world dataset has <= 32 vertices; the chain path costs
    // 1.295 µs/poly there vs ~0.1 µs for the pairwise sweep). The predicate
    // matches the chain leaf (rec_overlaps): strict proper crossing via
    // orient2d sign flips plus the full same-ring predicate, skipping
    // adjacent edges and testing the closing pair within each ring.
    if n <= crate::core::SMALL_RING_LINES {
        return has_no_intersections_small(lines);
    }

    let (chains, global_bbox) = build_mono_chains(lines);

    // Try fast grid path; fall back to R-tree if any cell gets too dense
    let grid_result = has_no_intersections_grid(&chains, lines, global_bbox);
    if let Some(result) = grid_result {
        return result;
    }

    // Fallback: R-tree
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

    // Per-cell pair tests with GLOBAL pair dedup: a chain pair (i, j) that
    // co-occurs in several cells (convex rings: every chain spans most
    // cells) was tested once PER cell - measured 2026-08-09 via hotpath:
    // rec_overlaps got 2.5M calls on the valid-polygon rows, ~198% of
    // wall, and a 5000-vertex circle's chain pairs were tested 4-9x
    // redundantly. compute_overlaps is deterministic and context-free, so
    // testing each pair once is decision-identical. Bit j of tested[i]
    // marks pair (i, j) done (cell contents are ascending in chain index,
    // so i < j always). The bitset is safe only when chain indices fit
    // u64: convex rings (the redundant-pair class) have ~4-8 chains;
    // complex rings with nc > 64 keep the per-cell behavior (their chains
    // are small and span few cells, so the redundancy is bounded anyway).
    // A mask of j >= 64 would silently skip pairs - never dedup by global
    // index without this guard.
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

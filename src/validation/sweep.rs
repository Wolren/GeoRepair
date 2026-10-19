//! Extracted from validation/core.rs on 2026-08-07 (file-size governance: core.rs was
//! 2540 lines; the cap is 800).

use crate::validation::core::*;
use alloc::boxed::Box;
use alloc::vec::Vec;
use geo::Coord;

/// Padded 2D bounding-box overlap for an edge pair (the R-tree envelope filter
/// equivalent: both boxes inflated by 1e-10 x max-dim, floored at 1e-10).
#[inline]
pub(crate) fn padded_bbox_overlap(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
) -> bool {
    let (lo_x, hi_x) = if a1.x < a2.x {
        (a1.x, a2.x)
    } else {
        (a2.x, a1.x)
    };
    let (lo_y, hi_y) = if a1.y < a2.y {
        (a1.y, a2.y)
    } else {
        (a2.y, a1.y)
    };
    let (lo_x2, hi_x2) = if b1.x < b2.x {
        (b1.x, b2.x)
    } else {
        (b2.x, b1.x)
    };
    let (lo_y2, hi_y2) = if b1.y < b2.y {
        (b1.y, b2.y)
    } else {
        (b2.y, b1.y)
    };
    let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
    let ext2 = (hi_x2 - lo_x2).abs().max((hi_y2 - lo_y2).abs()).max(1.0) * 1e-10;
    hi_x + ext >= lo_x2 - ext2
        && lo_x - ext <= hi_x2 + ext2
        && hi_y + ext >= lo_y2 - ext2
        && lo_y - ext <= hi_y2 + ext2
}

/// NaN handled upstream (finite check in check_ring_validity).
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

/// Run a closure against the radix-sort scratch buffers.
#[cfg(feature = "std")]
fn with_scratch<R>(f: impl FnOnce(&mut SweepScratch) -> R) -> R {
    SWEEP_SCRATCH.with(|s| f(&mut s.borrow_mut()))
}

#[cfg(not(feature = "std"))]
fn with_scratch<R>(f: impl FnOnce(&mut SweepScratch) -> R) -> R {
    f(&mut SweepScratch::default())
}

struct SweepScratch {
    /// Sort key per edge (sortable `lo_x`), maintained sorted with `order`.
    keys: Vec<u64>,
    /// Edge indices in ascending `lo_x` order.
    order: Vec<u32>,
    /// Incremental x-active set for the non-degenerate path.
    active: Vec<u32>,
    tmp_keys: Vec<u64>,
    tmp_order: Vec<u32>,
    counts: Box<[u32; 256]>,
    /// Padded bounds per ring edge: [lo_x, hi_x, lo_y, hi_y].
    spans: Vec<[f64; 4]>,
}

impl Default for SweepScratch {
    fn default() -> Self {
        SweepScratch {
            keys: Vec::new(),
            order: Vec::new(),
            active: Vec::new(),
            tmp_keys: Vec::new(),
            tmp_order: Vec::new(),
            counts: Box::new([0u32; 256]),
            spans: Vec::new(),
        }
    }
}

/// lsd radix sort of (keys, order) pairs, 8 bits x 8 passes.
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

/// Radix sort of (keys, order) using the tls scratch buffers (no caller borrow held).
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

/// Static uniform-grid index over a set of bounding boxes (`[lo_x, hi_x, lo_y, hi_y]`).
/// Measured 2026-10-17 on the figure-8 ladder: that degeneration was ~90 % of a
/// 4000-vertex noder call, split across three separate scans
/// (`sweep_ring_self_intersects`, `crossing_only`'s candidate loop. `run_full`'s
/// removed-family-vs-sweep loop), all reached through different flags but all the same
/// bug. `build` returns `None` when one cell holds more than [`GRID_MAX_CELL`] boxes.
pub(crate) struct SegGrid {
    min_x: f64,
    min_y: f64,
    inv_cell: f64,
    nx: usize,
    ny: usize,
    /// csr cell boundaries into `items`, length `nx * ny + 1`.
    offsets: Vec<u32>,
    /// Cell contents: positions in build order, ascending within a cell.
    items: Vec<u32>,
    /// Per-position dedup stamp; `seen[p] == epoch` means position `p` was already
    /// handed to the caller for this query.
    seen: Vec<u32>,
    epoch: u32,
}

/// Above this many boxes in one cell the grid is abandoned.
pub(crate) const GRID_MAX_CELL: usize = 64;

#[inline]
fn cell_range(lo: f64, hi: f64, min: f64, inv_cell: f64, nc: usize) -> (usize, usize) {
    let idx = |v: f64| {
        let q = (v - min) * inv_cell;
        if q.is_nan() {
            0
        } else {
            (q as isize).clamp(0, nc as isize - 1) as usize
        }
    };
    let (a, b) = (idx(lo), idx(hi));
    if a <= b { (a, b) } else { (b, a) }
}

impl SegGrid {
    pub(crate) fn build(boxes: &[[f64; 4]]) -> Option<SegGrid> {
        let m = boxes.len();
        if m == 0 {
            return None;
        }
        let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
        let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
        for b in boxes {
            min_x = min_x.min(b[0]);
            max_x = max_x.max(b[1]);
            min_y = min_y.min(b[2]);
            max_y = max_y.max(b[3]);
        }
        let span = (max_x - min_x).abs().max((max_y - min_y).abs());
        if span <= 0.0 || !span.is_finite() {
            return None;
        }
        let cell = (span / (m as f64).sqrt().ceil()).max(f64::MIN_POSITIVE);
        let inv_cell = 1.0 / cell;
        let nx = (((max_x - min_x) / cell).ceil() as usize).max(1);
        let ny = (((max_y - min_y) / cell).ceil() as usize).max(1);
        let nc = nx * ny;
        // measured ~2.6 us of pure allocation overhead on a 100-vertex
        let mut offsets: Vec<u32> = alloc::vec![0u32; nc + 1];
        for b in boxes {
            let (x0, x1) = cell_range(b[0], b[1], min_x, inv_cell, nx);
            let (y0, y1) = cell_range(b[2], b[3], min_y, inv_cell, ny);
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    let c = cy * nx + cx;
                    offsets[c + 1] += 1;
                    if offsets[c + 1] as usize > GRID_MAX_CELL {
                        return None;
                    }
                }
            }
        }
        for c in 0..nc {
            offsets[c + 1] += offsets[c];
        }
        let mut items: Vec<u32> = alloc::vec![0u32; offsets[nc] as usize];
        let mut cursor: Vec<u32> = offsets[..nc].to_vec();
        for (i, b) in boxes.iter().enumerate() {
            let (x0, x1) = cell_range(b[0], b[1], min_x, inv_cell, nx);
            let (y0, y1) = cell_range(b[2], b[3], min_y, inv_cell, ny);
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    let c = cy * nx + cx;
                    items[cursor[c] as usize] = i as u32;
                    cursor[c] += 1;
                }
            }
        }
        Some(SegGrid {
            min_x,
            min_y,
            inv_cell,
            nx,
            ny,
            offsets,
            items,
            seen: alloc::vec![0u32; m],
            epoch: 0,
        })
    }

    /// Visit every position whose box shares a grid cell with `b`, ascending, skipping
    /// `exclude` and without repeats.
    pub(crate) fn for_each_candidate<F: FnMut(usize)>(
        &mut self,
        b: [f64; 4],
        exclude: Option<usize>,
        mut f: F,
    ) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            for s in self.seen.iter_mut() {
                *s = 0;
            }
            self.epoch = 1;
        }
        let (x0, x1) = cell_range(b[0], b[1], self.min_x, self.inv_cell, self.nx);
        let (y0, y1) = cell_range(b[2], b[3], self.min_y, self.inv_cell, self.ny);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let c = cy * self.nx + cx;
                let (s, e) = (self.offsets[c] as usize, self.offsets[c + 1] as usize);
                for k in s..e {
                    let p = self.items[k] as usize;
                    if Some(p) == exclude || self.seen[p] == self.epoch {
                        continue;
                    }
                    self.seen[p] = self.epoch;
                    f(p);
                }
            }
        }
    }
}

/// Active-set size at which [`sweep_ring_self_intersects`] gives up and lets the caller
/// use the index or brute-force path.
const SWEEP_DENSE_ACTIVE_LIMIT: usize = 256;

/// Longest equal-`lo_x` run tolerated by the incremental active set in
/// [`sweep_ring_self_intersects`] before it switches to the static grid.
const SWEEP_DEGENERATE_RUN: usize = 32;

/// It returns Some(true) on an intersection, Some(false) when clean.
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
        let mut lo_run = 1usize;
        let mut lo_max_run = 1usize;
        let mut prev_lo_bits = 0u64;
        for i in 0..n {
            let a = ring[i];
            let b = ring[(i + 1).min(n)];
            let (lo_x, hi_x) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
            let (lo_y, hi_y) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
            let lo_bits = lo_x.to_bits();
            if i > 0 {
                if lo_bits == prev_lo_bits {
                    lo_run += 1;
                    if lo_run > lo_max_run {
                        lo_max_run = lo_run;
                    }
                } else {
                    lo_run = 1;
                }
            }
            prev_lo_bits = lo_bits;
            let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
            let k = sortable_u64(lo_x - ext);
            if i > 0 && k < keys[i - 1] {
                sorted = false;
            }
            keys.push(k);
            order.push(i as u32);
            spans.push([lo_x - ext, hi_x + ext, lo_y - ext, hi_y + ext]);
        }

        // scan at ~2.4 ns/vertex. run of segments shares one lo_x - a subdivided
        // vertical edge -because such a run can never retire
        if lo_max_run > SWEEP_DEGENERATE_RUN {
            return sweep_with_grid(spans, ring, n, eps, check_revisit, closed);
        }

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
            if active.len() > SWEEP_DENSE_ACTIVE_LIMIT {
                return None;
            }
            active.retain(|&p| spans[p as usize][1] >= cur[0]);
            for &p in &*active {
                let t = spans[p as usize];
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
                    a1,
                    a2,
                    b1,
                    b2,
                    eps,
                    &mut ambiguous,
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

/// No radix sort, because the grid does not care about x order.
fn sweep_with_grid(
    spans: &[[f64; 4]],
    ring: &[Coord<f64>],
    n: usize,
    eps: f64,
    check_revisit: bool,
    closed: bool,
) -> Option<bool> {
    let mut grid = SegGrid::build(spans)?;
    let mut hit = false;
    for r_i in 0..n {
        if hit {
            break;
        }
        let cur = spans[r_i];
        grid.for_each_candidate(cur, Some(r_i), |r_j| {
            if hit || r_j >= r_i {
                return;
            }
            let t = spans[r_j];
            if t[3] < cur[2] || t[2] > cur[3] {
                return;
            }
            if r_i.abs_diff(r_j) <= 1 {
                return;
            }
            let a1 = ring[r_i];
            let a2 = ring[(r_i + 1).min(n)];
            let b1 = ring[r_j];
            let b2 = ring[(r_j + 1).min(n)];
            let mut ambiguous = false;
            if crate::validation::edges::lean_pair_intersects(a1, a2, b1, b2, eps, &mut ambiguous) {
                hit = true;
                return;
            }
            if ambiguous
                && check_revisit
                && !(closed && (r_i == 0 && r_j == n - 1 || r_i == n - 1 && r_j == 0))
                && (a1 == b1 || a1 == b2 || a2 == b1 || a2 == b2)
            {
                hit = true;
            }
        });
    }
    if hit {
        return Some(true);
    }
    Some(false)
}

/// R-tree self-intersection check (the dense-ring fallback; kept exact - same predicate
/// and pair rules as the sweep).
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

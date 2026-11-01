//! Lean noding path for the crossing-only class.
//!
//! This covers the common non-simple line: proper crossings, endpoint revisits, and
//! vertex-on-edge contacts, with no collinear overlap between adjacent segments and no
//! near-coincident anchors. Anything else returns `Unsupported` and the caller falls
//! back to the general noder in the parent module.
//!
//! The parent path reaches the same result through a radix sort of every coordinate
//! plus a segment grid. Here the near-coincidence guard rides along with the candidate
//! pairs instead of a second sort, and the pair enumeration runs on one packed
//! per-segment record.

use alloc::vec;
use alloc::vec::Vec;

use geo::Coord;

use crate::validation::impls::segments_collinear_overlap;

use super::Hit;
use super::classify::classify;

/// Outcome of [`node_fast`].
pub(crate) enum Fast {
    /// No self-intersection: the caller returns the input unchanged.
    Simple,
    /// Noded simple chains.
    Noded(Vec<Vec<Coord<f64>>>),
    /// The lean path does not apply; the caller must use the general noder.
    Unsupported,
}

/// Per-segment record: padded bounds plus the grid cell range it occupies, packed so
/// the pair scan streams one array instead of four. The endpoints stay in the caller's
/// coordinate slice, indexed by segment number.
#[derive(Clone, Copy)]
struct Seg {
    lo_x: f64,
    hi_x: f64,
    lo_y: f64,
    hi_y: f64,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
}

/// True when the line runs monotonically in x or in y, which rules out a
/// self-intersection: non-adjacent segments then occupy disjoint ranges on that
/// axis and adjacent segments share a single vertex. A closed line is never
/// strictly monotone, so this cannot hide a closure contact.
fn monotone(coords: &[Coord<f64>]) -> bool {
    if coords.len() < 3 {
        return true;
    }
    let (mut inc_x, mut dec_x) = (true, true);
    let (mut inc_y, mut dec_y) = (true, true);
    for w in coords.windows(2) {
        let (p, q) = (w[0], w[1]);
        if q.x <= p.x {
            inc_x = false;
        }
        if q.x >= p.x {
            dec_x = false;
        }
        if q.y <= p.y {
            inc_y = false;
        }
        if q.y >= p.y {
            dec_y = false;
        }
        if !(inc_x || dec_x || inc_y || dec_y) {
            return false;
        }
    }
    inc_x || dec_x || inc_y || dec_y
}

/// Node a non-simple line without the sort-heavy guards of the general path.
pub(crate) fn node_fast(coords: &[Coord<f64>]) -> Fast {
    let n = coords.len() - 1;
    if n < 2 {
        return Fast::Simple;
    }
    // A line that is strictly monotone in x or in y cannot self-intersect: non-adjacent
    // segments then occupy disjoint ranges on that axis, and adjacent segments share a
    // single vertex. A closed line is never strictly monotone, so this cannot hide a
    // closure contact.
    if monotone(coords) {
        return Fast::Simple;
    }
    let (mut min_x, mut max_x) = (f64::MAX, f64::MIN);
    let (mut min_y, mut max_y) = (f64::MAX, f64::MIN);
    for c in coords {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let span = (max_x - min_x).abs().max((max_y - min_y).abs());
    if !span.is_finite() || span <= 0.0 {
        return Fast::Unsupported;
    }
    let eps = 1e-12 * span.max(1.0);
    let closed = coords[0] == coords[n];

    // Two vertices two apart within eps are an eps class the chain assembly cannot
    // separate. This guard needs only the coordinates, and it fires early on a retraced
    // curve, so it runs before anything is allocated.
    for i in 0..n - 1 {
        let p = coords[i];
        let q = coords[i + 2];
        if (p.x - q.x).abs() <= eps && (p.y - q.y).abs() <= eps {
            return Fast::Unsupported;
        }
    }
    if closed {
        let p = coords[n - 1];
        let q = coords[1];
        if (p.x - q.x).abs() <= eps && (p.y - q.y).abs() <= eps {
            return Fast::Unsupported;
        }
    }
    // Grid layout first: every segment's cell range is needed by the counting pass, the
    // filling pass and the query, so it is computed once, here.
    let mut grid = match Grid::new(min_x, min_y, max_x, max_y, n, eps) {
        Some(g) => g,
        None => {
            return Fast::Unsupported;
        }
    };
    // A segment shorter than eps has no unambiguous interior.
    let mut segs: Vec<Seg> = Vec::with_capacity(n);
    for i in 0..n {
        let a = coords[i];
        let b = coords[i + 1];
        if (b.x - a.x).abs() <= eps && (b.y - a.y).abs() <= eps {
            return Fast::Unsupported;
        }
        let (lo_x, hi_x) = (a.x.min(b.x), a.x.max(b.x));
        let (lo_y, hi_y) = (a.y.min(b.y), a.y.max(b.y));
        let (x0, x1) = grid.cells_x(lo_x, hi_x);
        let (y0, y1) = grid.cells_y(lo_y, hi_y);
        segs.push(Seg {
            lo_x,
            hi_x,
            lo_y,
            hi_y,
            x0: x0 as u32,
            x1: x1 as u32,
            y0: y0 as u32,
            y1: y1 as u32,
        });
    }

    // Adjacent collinear overlap: the general path owns it.
    for i in 0..n - 1 {
        if segments_collinear_overlap(coords[i], coords[i + 1], coords[i + 1], coords[i + 2], eps) {
            return Fast::Unsupported;
        }
    }
    if closed && segments_collinear_overlap(coords[n - 1], coords[n], coords[0], coords[1], eps) {
        return Fast::Unsupported;
    }

    let mut nodes: Vec<(u32, Coord<f64>)> = Vec::new();
    let mut revisits: Vec<Coord<f64>> = Vec::new();
    let ctx = ScanCtx {
        coords,
        segs: &segs,
        n,
        closed,
        eps,
        bbox_w: max_x - min_x,
    };
    if !scan_pairs(&ctx, &mut grid, &mut nodes, &mut revisits) {
        return Fast::Unsupported;
    }
    if nodes.is_empty() && revisits.is_empty() {
        return Fast::Simple;
    }
    let out = assemble(coords, n, closed, eps, &nodes, &revisits);
    match out {
        Some(chains) => Fast::Noded(chains),
        None => Fast::Unsupported,
    }
}

/// Everything the pair scan reads, so the scan takes one reference instead of six
/// arguments.
struct ScanCtx<'a> {
    coords: &'a [Coord<f64>],
    segs: &'a [Seg],
    n: usize,
    closed: bool,
    eps: f64,
    bbox_w: f64,
}

/// Collect the crossing nodes and revisit points. Returns false when a pair needs the
/// general path.
fn scan_pairs(
    ctx: &ScanCtx<'_>,
    grid: &mut Grid,
    nodes: &mut Vec<(u32, Coord<f64>)>,
    revisits: &mut Vec<Coord<f64>>,
) -> bool {
    let (coords, segs, n, closed, eps) = (ctx.coords, ctx.segs, ctx.n, ctx.closed, ctx.eps);
    // A few segments spanning most of the box make every cell dense; the general path
    // handles that shape better.
    {
        let bx = ctx.bbox_w.abs().max(f64::MIN_POSITIVE);
        let mut wide = 0usize;
        for s in segs {
            if s.hi_x - s.lo_x >= 0.9 * bx {
                wide += 1;
                if wide >= 4 {
                    return false;
                }
            }
        }
    }

    if !grid.fill(segs) {
        return false;
    }

    let mut ok = true;
    for j in 0..n {
        if !ok {
            return false;
        }
        let (aj, bj) = (coords[j], coords[j + 1]);
        let sj = &segs[j];
        let (jlo_x, jhi_x, jlo_y, jhi_y) = (sj.lo_x, sj.hi_x, sj.lo_y, sj.hi_y);
        grid.epoch = grid.epoch.wrapping_add(1);
        if grid.epoch == 0 {
            grid.seen.fill(0);
            grid.epoch = 1;
        }
        'cells: for cy in sj.y0..=sj.y1 {
            for cx in sj.x0..=sj.x1 {
                let cell = cy as usize * grid.nx + cx as usize;
                let (lo, hi) = (grid.offsets[cell] as usize, grid.offsets[cell + 1] as usize);
                for &i in &grid.items[lo..hi] {
                    let i = i as usize;
                    if i >= j || grid.seen[i] == grid.epoch {
                        continue;
                    }
                    grid.seen[i] = grid.epoch;
                    if j == i + 1 || (closed && i == 0 && j == n - 1) {
                        continue;
                    }
                    let si = &segs[i];
                    if si.hi_x < jlo_x - eps
                        || jhi_x < si.lo_x - eps
                        || si.hi_y < jlo_y - eps
                        || jhi_y < si.lo_y - eps
                    {
                        continue;
                    }
                    let (ai, bi) = (coords[i], coords[i + 1]);
                    // Two distinct anchors within eps cannot be separated by the
                    // assembly, so that shape belongs to the general path.
                    if near(ai, aj, eps)
                        || near(ai, bj, eps)
                        || near(bi, aj, eps)
                        || near(bi, bj, eps)
                    {
                        ok = false;
                        break 'cells;
                    }
                    match classify(ai, bi, aj, bj, eps) {
                        Hit::None => {}
                        Hit::Shared => {
                            let v = if ai == aj || ai == bj { ai } else { bi };
                            revisits.push(v);
                        }
                        Hit::Cross(pt) => {
                            nodes.push((i as u32, pt));
                            nodes.push((j as u32, pt));
                        }
                        Hit::VertexOnEdge(v) => {
                            let seg = if v == ai || v == bi { j } else { i };
                            nodes.push((seg as u32, v));
                        }
                        Hit::Collinear => {
                            ok = false;
                            break 'cells;
                        }
                    }
                }
            }
        }
    }
    ok
}

/// Uniform 2-D bucket grid over the segment bounds, built as CSR: one counting pass,
/// one fill pass. A cell above `MAX_CELL` items means the shape is better served by the
/// general path.
struct Grid {
    min_x: f64,
    min_y: f64,
    inv_cell: f64,
    nx: usize,
    ny: usize,
    offsets: Vec<u32>,
    items: Vec<u32>,
    seen: Vec<u32>,
    epoch: u32,
}

impl Grid {
    const MAX_CELL: u32 = 64;

    /// Cell layout for a segment set inside the given bounds, widened by `eps`. The
    /// per-segment cell ranges are computed by the caller from this layout and reused by
    /// the counting pass, the filling pass and the query.
    fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64, m: usize, eps: f64) -> Option<Grid> {
        let min_x = min_x - eps;
        let min_y = min_y - eps;
        let max_x = max_x + eps;
        let max_y = max_y + eps;
        let span = (max_x - min_x).abs().max((max_y - min_y).abs());
        if !span.is_finite() || span <= 0.0 {
            return None;
        }
        // Cell from the occupied area, not the longer side: an elongated box (a line that
        // runs mostly along one axis) otherwise collapses to a single row of cells and
        // every query scans a large fraction of the segments.
        let (w, h) = (max_x - min_x, max_y - min_y);
        let area = (w * h).max(f64::MIN_POSITIVE);
        let mf = m as f64;
        let mut cell = (area / mf).sqrt().max(span / (4.0 * mf));
        let (mut nx, mut ny) = Self::dims(w, h, cell);
        let max_cells = 8 * m + 64;
        if nx * ny > max_cells {
            cell *= ((nx * ny) as f64 / max_cells as f64).sqrt();
            let d = Self::dims(w, h, cell);
            nx = d.0;
            ny = d.1;
        }
        let nc = nx * ny;
        Some(Grid {
            min_x,
            min_y,
            inv_cell: 1.0 / cell,
            nx,
            ny,
            offsets: alloc::vec![0u32; nc + 1],
            items: Vec::new(),
            seen: alloc::vec![0u32; m],
            epoch: 0,
        })
    }

    /// CSR fill from the per-segment cell ranges. False when a cell is too dense, which
    /// means the shape is better served by the general path.
    fn fill(&mut self, segs: &[Seg]) -> bool {
        let nx = self.nx;
        for s in segs {
            for cy in s.y0..=s.y1 {
                for cx in s.x0..=s.x1 {
                    let c = cy as usize * nx + cx as usize;
                    self.offsets[c + 1] += 1;
                    if self.offsets[c + 1] > Self::MAX_CELL {
                        return false;
                    }
                }
            }
        }
        let nc = self.nx * self.ny;
        for c in 0..nc {
            self.offsets[c + 1] += self.offsets[c];
        }
        let total = self.offsets[nc] as usize;
        self.items = alloc::vec![0u32; total];
        for (i, s) in segs.iter().enumerate() {
            for cy in s.y0..=s.y1 {
                for cx in s.x0..=s.x1 {
                    let c = cy as usize * nx + cx as usize;
                    let slot = self.offsets[c] as usize;
                    self.items[slot] = i as u32;
                    self.offsets[c] = slot as u32 + 1;
                }
            }
        }
        // The fill pass left the start of cell c+1 at index c: shift right to restore.
        for c in (1..=nc).rev() {
            self.offsets[c] = self.offsets[c - 1];
        }
        self.offsets[0] = 0;
        true
    }

    #[inline]
    fn dims(w: f64, h: f64, cell: f64) -> (usize, usize) {
        (
            ((w / cell).ceil() as usize).max(1),
            ((h / cell).ceil() as usize).max(1),
        )
    }

    #[inline]
    fn idx(&self, v: f64, min: f64, nc: usize) -> usize {
        let q = (v - min) * self.inv_cell;
        if q <= 0.0 {
            return 0;
        }
        let i = q as usize;
        if i >= nc { nc - 1 } else { i }
    }

    #[inline]
    fn cells_x(&self, lo: f64, hi: f64) -> (usize, usize) {
        (
            self.idx(lo, self.min_x, self.nx),
            self.idx(hi, self.min_x, self.nx),
        )
    }

    #[inline]
    fn cells_y(&self, lo: f64, hi: f64) -> (usize, usize) {
        (
            self.idx(lo, self.min_y, self.ny),
            self.idx(hi, self.min_y, self.ny),
        )
    }
}

/// Distinct points within eps (Chebyshev). Exact equality is not an eps class.
#[inline]
fn near(p: Coord<f64>, q: Coord<f64>, eps: f64) -> bool {
    (p.x.to_bits() != q.x.to_bits() || p.y.to_bits() != q.y.to_bits())
        && (p.x - q.x).abs() <= eps
        && (p.y - q.y).abs() <= eps
}

/// Split the line at the collected nodes and walk the resulting chains.
fn assemble(
    coords: &[Coord<f64>],
    n: usize,
    closed: bool,
    eps: f64,
    nodes: &[(u32, Coord<f64>)],
    revisits: &[Coord<f64>],
) -> Option<Vec<Vec<Coord<f64>>>> {
    // Group the splits by segment in one sorted pass. The alternative, a
    // `Vec<Vec<Coord>>` with one entry per segment, costs an allocation and a 24 KB
    // touch per call for the handful of segments that carry a node.
    let mut by_seg: Vec<(u32, Coord<f64>)> = nodes.to_vec();
    by_seg.sort_unstable_by_key(|&(seg, _)| seg);
    let mut breaks: Vec<(f64, f64)> = Vec::with_capacity(nodes.len() + revisits.len());
    for &(_, p) in nodes {
        breaks.push((p.x, p.y));
    }
    for c in revisits {
        breaks.push((c.x, c.y));
    }
    breaks.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    breaks.dedup_by(|a, b| a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits());

    let mut starts: Vec<Coord<f64>> = Vec::with_capacity(n + nodes.len() + 1);
    let mut k = 0usize;
    for seg in 0..n {
        let a = coords[seg];
        let b = coords[seg + 1];
        starts.push(a);
        let begin = k;
        while k < by_seg.len() && by_seg[k].0 as usize == seg {
            k += 1;
        }
        if begin == k {
            continue;
        }
        let group = &mut by_seg[begin..k];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len = (dx * dx + dy * dy).sqrt();
        group.sort_by(|p, q| {
            let tp = ((p.1.x - a.x) * dx + (p.1.y - a.y) * dy) / len;
            let tq = ((q.1.x - a.x) * dx + (q.1.y - a.y) * dy) / len;
            tp.partial_cmp(&tq).unwrap_or(core::cmp::Ordering::Equal)
        });
        let mut prev_dist = 0.0f64;
        let mut prev_pt: Option<Coord<f64>> = None;
        for &(_, p) in group.iter() {
            if let Some(q) = prev_pt
                && q.x.to_bits() == p.x.to_bits()
                && q.y.to_bits() == p.y.to_bits()
            {
                continue;
            }
            prev_pt = Some(p);
            let d = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len;
            if d < -eps || d > len + eps {
                return None;
            }
            if (d - prev_dist).abs() <= eps || (len - d).abs() <= eps {
                return None;
            }
            starts.push(p);
            prev_dist = d;
        }
    }
    starts.push(coords[n]);
    let m = starts.len() - 1;
    let is_break = |k: usize| {
        breaks
            .binary_search_by(|e| {
                e.0.total_cmp(&starts[k].x)
                    .then(e.1.total_cmp(&starts[k].y))
            })
            .is_ok()
    };
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
        "lean-path chains must be simple"
    );
    Some(chains)
}

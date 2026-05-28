use std::collections::{HashMap, HashSet};
#[cfg(feature = "parallel")]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::MakeValidError;
use crate::orient::{orient2d, orient2d_fast};
use crate::snap;
use geo::{Coord, Line};

#[derive(Debug)]
pub(crate) struct PreparedLines {
    pub lines: Vec<Line<f64>>,
}

pub(crate) fn prepare_lines(lines: Vec<Line<f64>>) -> Result<PreparedLines, MakeValidError> {
    for line in &lines {
        for c in [line.start, line.end] {
            if c.x.is_nan() || c.y.is_nan() {
                return Err(MakeValidError::CoordinateIsNaN { idx: 0 });
            }
        }
    }
    // Use grid-hashed spatial lookup for O(n) snap dedup
    let snap_radius = 1e-8;
    let cell_size = snap_radius * 2.0;
    let mut grid: HashMap<(i64, i64), Coord<f64>> = HashMap::new();
    let mut snapped = Vec::with_capacity(lines.len());
    for mut line in lines {
        line.start = snap_or_push_grid(line.start, &mut grid, snap_radius, cell_size);
        line.end = snap_or_push_grid(line.end, &mut grid, snap_radius, cell_size);
        if line.start != line.end {
            snapped.push(line);
        }
    }
    odd_even_filter(&mut snapped);
    let mut split = split_segments(snapped)?;
    odd_even_filter(&mut split);
    Ok(PreparedLines { lines: split })
}

fn grid_key(coord: Coord<f64>, cell_size: f64) -> (i64, i64) {
    let x = coord.x / cell_size;
    let y = coord.y / cell_size;
    let xi = if x.is_finite() {
        x.floor().clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0i64
    };
    let yi = if y.is_finite() {
        y.floor().clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0i64
    };
    (xi, yi)
}

fn snap_or_push_grid(
    coord: Coord<f64>,
    grid: &mut HashMap<(i64, i64), Coord<f64>>,
    snap_radius: f64,
    cell_size: f64,
) -> Coord<f64> {
    let key = grid_key(coord, cell_size);
    // Check this cell and all 8 neighbors (use saturating arithmetic for edge cells)
    for dc in -1..=1i64 {
        for dr in -1..=1i64 {
            if let Some(&existing) = grid.get(&(key.0.saturating_add(dc), key.1.saturating_add(dr)))
            {
                let dx = coord.x - existing.x;
                let dy = coord.y - existing.y;
                if dx * dx + dy * dy <= snap_radius {
                    return existing;
                }
            }
        }
    }
    grid.insert(key, coord);
    coord
}

fn quadrant(x: f64, y: f64) -> u8 {
    if x > 0.0 {
        if y >= 0.0 {
            0
        } else {
            1
        }
    } else if x < 0.0 {
        if y > 0.0 {
            3
        } else {
            2
        }
    } else {
        if y > 0.0 {
            0
        } else {
            2
        }
    }
}

struct MonoChain {
    start: usize,
    end: usize,
    quad: u8,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    ring_id: u32,
    ring_start: usize,
    ring_len: usize,
}

impl MonoChain {
    fn sub_aabb(&self, lines: &[Line<f64>], s: usize, e: usize) -> (f64, f64, f64, f64) {
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

fn build_mono_chains(lines: &[Line<f64>]) -> Vec<MonoChain> {
    let n = lines.len();
    if n == 0 {
        return vec![];
    }

    // Detect ring boundaries: within a ring, segment i connects to segment i-1.
    // A segment whose start != previous segment's end starts a new ring.
    let mut ring_bounds = Vec::new();
    let mut ring_s = 0usize;
    for i in 1..n {
        if lines[i].start != lines[i - 1].end {
            ring_bounds.push((ring_s, i));
            ring_s = i;
        }
    }
    ring_bounds.push((ring_s, n));
    let ring_buf = ring_bounds.as_slice();

    let l0 = &lines[0];
    let dx = l0.end.x - l0.start.x;
    let dy = l0.end.y - l0.start.y;
    let mut prev_quad = quadrant(dx, dy);
    let mut start = 0usize;
    let mut min_x = l0.start.x.min(l0.end.x);
    let mut max_x = l0.start.x.max(l0.end.x);
    let mut min_y = l0.start.y.min(l0.end.y);
    let mut max_y = l0.start.y.max(l0.end.y);

    let (mut ring_start, mut ring_end) = ring_buf[0];
    let mut ring_idx = 0u32;
    let mut chains = Vec::new();

    for i in 1..n {
        // Force chain break at ring boundary
        let at_ring_boundary = i == ring_end;

        let line = &lines[i];
        min_x = min_x.min(line.start.x).min(line.end.x);
        max_x = max_x.max(line.start.x).max(line.end.x);
        min_y = min_y.min(line.start.y).min(line.end.y);
        max_y = max_y.max(line.start.y).max(line.end.y);

        let dx = line.end.x - line.start.x;
        let dy = line.end.y - line.start.y;
        let cur_quad = quadrant(dx, dy);
        if at_ring_boundary || cur_quad != prev_quad {
            let ring_len = ring_end - ring_start;
            chains.push(MonoChain {
                start,
                end: i,
                quad: prev_quad,
                min_x,
                min_y,
                max_x,
                max_y,
                ring_id: ring_idx,
                ring_start,
                ring_len,
            });
            start = i;
            prev_quad = cur_quad;
            let l = &lines[i];
            min_x = l.start.x.min(l.end.x);
            max_x = l.start.x.max(l.end.x);
            min_y = l.start.y.min(l.end.y);
            max_y = l.start.y.max(l.end.y);

            if at_ring_boundary {
                ring_idx += 1;
                let rb = ring_buf[ring_idx as usize];
                ring_start = rb.0;
                ring_end = rb.1;
            }
        }
    }
    let ring_len = ring_end - ring_start;
    chains.push(MonoChain {
        start,
        end: n,
        quad: prev_quad,
        min_x,
        min_y,
        max_x,
        max_y,
        ring_id: ring_idx,
        ring_start,
        ring_len,
    });
    chains
}

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
        if mc1.ring_id == mc2.ring_id {
            if j == i + 1 || j + 1 == i {
                return false;
            }
            let ring_first = mc1.ring_start;
            let ring_last = mc1.ring_start + mc1.ring_len - 1;
            if (i == ring_first && j == ring_last) || (j == ring_first && i == ring_last) {
                return false;
            }
        }
        let li = &lines[i];
        let lj = &lines[j];
        let o1 = orient2d_fast(li.start, li.end, lj.start);
        let o2 = orient2d_fast(li.start, li.end, lj.end);
        let o3 = orient2d_fast(lj.start, lj.end, li.start);
        let o4 = orient2d_fast(lj.start, lj.end, li.end);
        return (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0);
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

fn build_chain_grid(chains: &[MonoChain]) -> (HashMap<(i64, i64), Vec<usize>>, f64, f64, f64, f64) {
    let nc = chains.len();
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for mc in chains {
        min_x = min_x.min(mc.min_x);
        min_y = min_y.min(mc.min_y);
        max_x = max_x.max(mc.max_x);
        max_y = max_y.max(mc.max_y);
    }
    let scale = ((max_x - min_x) + (max_y - min_y)).max(1e-12);
    let cell_size = (scale / (nc as f64).sqrt()).max(1e-12);
    let inv_cell = 1.0 / cell_size;

    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::with_capacity(nc * 2);
    for (ci, mc) in chains.iter().enumerate() {
        let ix0 = ((mc.min_x - min_x) * inv_cell).floor() as i64;
        let iy0 = ((mc.min_y - min_y) * inv_cell).floor() as i64;
        let ix1 = ((mc.max_x - min_x) * inv_cell).floor() as i64;
        let iy1 = ((mc.max_y - min_y) * inv_cell).floor() as i64;
        for ix in ix0..=ix1 {
            for iy in iy0..=iy1 {
                grid.entry((ix, iy)).or_default().push(ci);
            }
        }
    }
    (grid, cell_size, min_x, min_y, inv_cell)
}

pub(crate) fn has_no_intersections(lines: &[Line<f64>]) -> bool {
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

    let chains = build_mono_chains(lines);
    let nc = chains.len();
    if nc <= 1 {
        return true;
    }

    let (grid, _cell_size, _min_x, _min_y, _inv_cell) = build_chain_grid(&chains);

    let do_parallel = nc >= 200;

    #[cfg(feature = "parallel")]
    if do_parallel {
        use rayon::prelude::*;
        let found = AtomicBool::new(false);
        let grid_vec: Vec<_> = grid.into_iter().collect();
        grid_vec.par_iter().for_each(|(_, cell_chains)| {
            if found.load(Ordering::Relaxed) {
                return;
            }
            let nc = cell_chains.len();
            if nc <= 1 {
                return;
            }
            for ii in 0..nc {
                let ci = cell_chains[ii];
                let mc1 = &chains[ci];
                for jj in (ii + 1)..nc {
                    if found.load(Ordering::Relaxed) {
                        return;
                    }
                    let cj = cell_chains[jj];
                    let mc2 = &chains[cj];
                    if compute_overlaps(lines, mc1, mc2) {
                        found.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            }
        });
        return !found.load(Ordering::Relaxed);
    }

    for (_, cell_chains) in &grid {
        let nc = cell_chains.len();
        if nc <= 1 {
            continue;
        }
        for ii in 0..nc {
            let ci = cell_chains[ii];
            let mc1 = &chains[ci];
            for jj in (ii + 1)..nc {
                let cj = cell_chains[jj];
                let mc2 = &chains[cj];
                if compute_overlaps(lines, mc1, mc2) {
                    return false;
                }
            }
        }
    }
    true
}

pub(crate) fn odd_even_filter(lines: &mut Vec<Line<f64>>) {
    for line in lines.iter_mut() {
        if coord_greater(line.start, line.end) {
            std::mem::swap(&mut line.start, &mut line.end);
        }
    }
    lines.sort_by(|a, b| coord_cmp(a.start, b.start).then(coord_cmp(a.end, b.end)));
    let mut result = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let start = i;
        while i < lines.len() && lines[i] == lines[start] {
            i += 1;
        }
        if (i - start) % 2 == 1 {
            result.push(lines[start]);
        }
    }
    *lines = result;
}

fn coord_greater(a: Coord<f64>, b: Coord<f64>) -> bool {
    a.x > b.x || (a.x == b.x && a.y > b.y)
}

fn coord_cmp(a: Coord<f64>, b: Coord<f64>) -> std::cmp::Ordering {
    a.x.partial_cmp(&b.x)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
}

fn on_segment(a: Coord<f64>, b: Coord<f64>, c: Coord<f64>, eps: f64) -> bool {
    // Must be collinear
    if orient2d(a, b, c).abs() > eps {
        return false;
    }
    let minx = a.x.min(b.x) - eps;
    let maxx = a.x.max(b.x) + eps;
    let miny = a.y.min(b.y) - eps;
    let maxy = a.y.max(b.y) + eps;
    c.x >= minx && c.x <= maxx && c.y >= miny && c.y <= maxy
}

/// Returns the intersection point of two segments if they properly cross.
/// For collinear overlaps, returns the endpoints of the overlapping portion.
fn segment_intersection(
    a: Coord<f64>,
    b: Coord<f64>,
    c: Coord<f64>,
    d: Coord<f64>,
) -> Option<Vec<Coord<f64>>> {
    let eps = 1e-12;
    let o1 = orient2d(a, b, c);
    let o2 = orient2d(a, b, d);
    let o3 = orient2d(c, d, a);
    let o4 = orient2d(c, d, b);

    // Proper intersection (segments cross at interior points)
    if o1.abs() > eps && o2.abs() > eps && o3.abs() > eps && o4.abs() > eps {
        let s1 = o1.signum() != o2.signum();
        let s2 = o3.signum() != o4.signum();
        if s1 && s2 {
            let denom = (b.x - a.x) * (d.y - c.y) - (b.y - a.y) * (d.x - c.x);
            if denom.abs() < eps {
                return None;
            }
            let t = ((c.x - a.x) * (d.y - c.y) - (c.y - a.y) * (d.x - c.x)) / denom;
            let pt = Coord {
                x: a.x + t * (b.x - a.x),
                y: a.y + t * (b.y - a.y),
            };
            return Some(vec![pt]);
        }
        return None;
    }

    // Collinear or touching cases
    let collinear = o1.abs() < eps && o2.abs() < eps;
    if !collinear {
        // Endpoint touching — check if any endpoint lies on the other segment
        if o1.abs() < eps && on_segment(a, b, c, eps) {
            return Some(vec![c]);
        }
        if o2.abs() < eps && on_segment(a, b, d, eps) {
            return Some(vec![d]);
        }
        if o3.abs() < eps && on_segment(c, d, a, eps) {
            return Some(vec![a]);
        }
        if o4.abs() < eps && on_segment(c, d, b, eps) {
            return Some(vec![b]);
        }
        return None;
    }

    // Collinear overlap
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 < eps {
        return None;
    }
    let t_c = ((c.x - a.x) * dx + (c.y - a.y) * dy) / len2;
    let t_d = ((d.x - a.x) * dx + (d.y - a.y) * dy) / len2;
    let seg1_start = 0.0f64;
    let seg1_end = 1.0f64;
    let seg2_start = t_c.min(t_d);
    let seg2_end = t_c.max(t_d);
    let overlap_start = seg1_start.max(seg2_start);
    let overlap_end = seg1_end.min(seg2_end);
    if overlap_end - overlap_start <= eps {
        return None;
    }
    let p_start = Coord {
        x: a.x + overlap_start * dx,
        y: a.y + overlap_start * dy,
    };
    let p_end = Coord {
        x: a.x + overlap_end * dx,
        y: a.y + overlap_end * dy,
    };
    Some(vec![p_start, p_end])
}

struct SegGrid {
    cells: Vec<Vec<usize>>,
}

impl SegGrid {
    fn build(lines: &[Line<f64>]) -> SegGrid {
        let n = lines.len();
        if n == 0 {
            return SegGrid { cells: Vec::new() };
        }
        let (mut min_x, mut max_x) = (f64::MAX, f64::MIN);
        let (mut min_y, mut max_y) = (f64::MAX, f64::MIN);
        let mut total_len = 0.0f64;
        for line in lines {
            min_x = min_x.min(line.start.x).min(line.end.x);
            max_x = max_x.max(line.start.x).max(line.end.x);
            min_y = min_y.min(line.start.y).min(line.end.y);
            max_y = max_y.max(line.start.y).max(line.end.y);
            let dx = line.end.x - line.start.x;
            let dy = line.end.y - line.start.y;
            total_len += (dx * dx + dy * dy).sqrt();
        }
        let avg_len = total_len / n as f64;
        let cell_size = if avg_len.is_finite() && avg_len > 1e-12 {
            avg_len * 2.0
        } else {
            let span = (max_x - min_x).max(max_y - min_y);
            if span > 1e-12 {
                (span / 100.0).max(1e-8)
            } else {
                return SegGrid {
                    cells: vec![(0..n).collect()],
                };
            }
        };
        let pad = cell_size * 1e-4;
        let x_span = (max_x - min_x + pad).max(0.0);
        let y_span = (max_y - min_y + pad).max(0.0);
        let cols = ((x_span / cell_size).ceil() as usize).max(1);
        let rows = ((y_span / cell_size).ceil() as usize).max(1);
        let mut cells = vec![Vec::new(); cols * rows];

        for (i, line) in lines.iter().enumerate() {
            let x1 = line.start.x.min(line.end.x);
            let x2 = line.start.x.max(line.end.x);
            let y1 = line.start.y.min(line.end.y);
            let y2 = line.start.y.max(line.end.y);
            let cx_min = ((x1 - min_x) / cell_size).floor() as usize;
            let cx_max = ((x2 - min_x) / cell_size).floor() as usize;
            let cy_min = ((y1 - min_y) / cell_size).floor() as usize;
            let cy_max = ((y2 - min_y) / cell_size).floor() as usize;
            for cy in cy_min.min(rows - 1)..=cy_max.min(rows - 1) {
                let start = cy * cols;
                let row_end = start + cx_max.min(cols - 1);
                for idx in start + cx_min.min(cols - 1)..=row_end {
                    cells[idx].push(i);
                }
            }
        }
        SegGrid { cells }
    }
}

fn split_segments(lines: Vec<Line<f64>>) -> Result<Vec<Line<f64>>, MakeValidError> {
    let n = lines.len();
    let mut split_points: Vec<Vec<(f64, Coord<f64>)>> = vec![Vec::new(); n];

    let grid = SegGrid::build(&lines);
    let mut seen = HashSet::new();

    for cell in &grid.cells {
        if cell.len() < 2 {
            continue;
        }
        for idx_a in 0..cell.len() {
            let i = cell[idx_a];
            let li = &lines[i];
            for idx_b in (idx_a + 1)..cell.len() {
                let j = cell[idx_b];
                if j <= i {
                    continue;
                }
                let key = (i as u64) << 32 | (j as u64);
                if !seen.insert(key) {
                    continue;
                }
                let lj = &lines[j];
                if let Some(pts) = segment_intersection(li.start, li.end, lj.start, lj.end) {
                    for pt in pts {
                        let is_end_i = (pt == li.start) || (pt == li.end);
                        if !is_end_i {
                            let ti = project_param(li, pt);
                            if ti > 0.0 && ti < 1.0 {
                                split_points[i].push((ti, pt));
                            }
                        }
                        let is_end_j = (pt == lj.start) || (pt == lj.end);
                        if !is_end_j {
                            let tj = project_param(lj, pt);
                            if tj > 0.0 && tj < 1.0 {
                                split_points[j].push((tj, pt));
                            }
                        }
                    }
                }
            }
        }
    }

    let eps_param = 1e-14;
    let mut result = Vec::new();
    for i in 0..n {
        let line = lines[i];
        let mut splits: Vec<(f64, Coord<f64>)> = std::mem::take(&mut split_points[i]);
        splits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        splits.dedup_by(|a, b| (a.0 - b.0).abs() < eps_param);

        let mut prev_t = 0.0f64;
        let mut prev_pt = line.start;
        for &(t, pt) in &splits {
            if (t - prev_t).abs() < eps_param {
                continue;
            }
            let dx = pt.x - prev_pt.x;
            let dy = pt.y - prev_pt.y;
            if dx * dx + dy * dy > 1e-20 {
                result.push(Line::new(prev_pt, pt));
            }
            prev_t = t;
            prev_pt = pt;
        }
        let dx = line.end.x - prev_pt.x;
        let dy = line.end.y - prev_pt.y;
        if dx * dx + dy * dy > 1e-20 {
            result.push(Line::new(prev_pt, line.end));
        }
    }

    Ok(snap::snap_lines(result))
}

fn project_param(line: &Line<f64>, pt: Coord<f64>) -> f64 {
    let dx = line.end.x - line.start.x;
    let dy = line.end.y - line.start.y;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-30 {
        return 0.0;
    }
    ((pt.x - line.start.x) * dx + (pt.y - line.start.y) * dy) / len2
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------
    // odd_even_filter
    // -------------------------------

    #[test]
    fn test_odd_even_filter_empty() {
        let mut lines = Vec::new();
        odd_even_filter(&mut lines);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_odd_even_filter_single() {
        let mut lines = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        )];
        odd_even_filter(&mut lines);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_odd_even_filter_even_duplicates() {
        let mut lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
        ];
        odd_even_filter(&mut lines);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_odd_even_filter_odd_duplicates() {
        let l = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
        let mut lines = vec![l, l, l];
        odd_even_filter(&mut lines);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], l);
    }

    #[test]
    fn test_odd_even_filter_normalizes_orientation() {
        let l = Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 0.0, y: 0.0 });
        let mut lines = vec![l];
        odd_even_filter(&mut lines);
        // Start should be < end lexicographically
        assert!(
            lines[0].start.x < lines[0].end.x
                || (lines[0].start.x == lines[0].end.x && lines[0].start.y < lines[0].end.y)
        );
    }

    // -------------------------------
    // segment_intersection
    // -------------------------------

    #[test]
    fn test_segment_intersection_proper_crossing() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 2.0, y: 0.0 },
        );
        assert!(result.is_some());
        let pts = result.unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].x - 1.0).abs() < 1e-12);
        assert!((pts[0].y - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_segment_intersection_no_intersection() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 3.0, y: 3.0 },
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_intersection_endpoint_touching() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 1.0 },
        );
        assert!(result.is_some());
        let pts = result.unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].x - 1.0).abs() < 1e-12);
        assert_eq!(pts[0].y, 0.0);
    }

    #[test]
    fn test_segment_intersection_t_junction() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 2.0 },
        );
        assert!(result.is_some());
        let pts = result.unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].x - 1.0).abs() < 1e-12);
        assert_eq!(pts[0].y, 0.0);
    }

    #[test]
    fn test_segment_intersection_collinear_overlap() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 3.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 4.0, y: 0.0 },
        );
        assert!(result.is_some());
        let pts = result.unwrap();
        assert!(pts.len() >= 2);
    }

    #[test]
    fn test_segment_intersection_collinear_no_overlap() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 3.0, y: 0.0 },
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_intersection_parallel_non_intersecting() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_intersection_degenerate_zero_length() {
        let result = segment_intersection(
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
        );
        assert!(result.is_none());
    }

    // -------------------------------
    // on_segment
    // -------------------------------

    #[test]
    fn test_on_segment_midpoint() {
        assert!(on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            1e-12,
        ));
    }

    #[test]
    fn test_on_segment_endpoint() {
        assert!(on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            1e-12,
        ));
    }

    #[test]
    fn test_on_segment_not_on_segment() {
        assert!(!on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 3.0, y: 0.0 },
            1e-12,
        ));
    }

    #[test]
    fn test_on_segment_above() {
        assert!(!on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            1e-12,
        ));
    }

    #[test]
    fn test_on_segment_near_miss() {
        assert!(!on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 1e-11 },
            1e-12,
        ));
    }

    // -------------------------------
    // coord_greater / coord_cmp
    // -------------------------------

    #[test]
    fn test_coord_greater_x() {
        assert!(coord_greater(
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
        ));
        assert!(!coord_greater(
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
        ));
    }

    #[test]
    fn test_coord_greater_y_tiebreaker() {
        assert!(coord_greater(
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 1.0, y: 1.0 },
        ));
        assert!(!coord_greater(
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 2.0 },
        ));
    }

    #[test]
    fn test_coord_greater_equal() {
        assert!(!coord_greater(
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
        ));
    }

    // -------------------------------
    // project_param
    // -------------------------------

    #[test]
    fn test_project_param_midpoint() {
        let line = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 });
        let t = project_param(&line, Coord { x: 1.0, y: 1.0 });
        assert!((t - 0.5).abs() < 1e-12);
    }

    #[test]
    fn test_project_param_start() {
        let line = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 3.0, y: 4.0 });
        let t = project_param(&line, Coord { x: 1.0, y: 2.0 });
        assert!((t - 0.0).abs() < 1e-12);
    }

    #[test]
    fn test_project_param_end() {
        let line = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 3.0, y: 4.0 });
        let t = project_param(&line, Coord { x: 3.0, y: 4.0 });
        assert!((t - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_project_param_zero_length_line() {
        let line = Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 });
        let t = project_param(&line, Coord { x: 1.0, y: 1.0 });
        assert_eq!(t, 0.0);
    }

    #[test]
    fn test_project_param_off_line() {
        let line = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 });
        let t = project_param(&line, Coord { x: 1.0, y: 1.0 });
        assert!((t - 0.5).abs() < 1e-12);
    }

    // -------------------------------
    // snap_or_push_grid
    // -------------------------------

    #[test]
    fn test_snap_or_push_new() {
        let mut grid = std::collections::HashMap::new();
        let snap_radius = 1e-8;
        let cell_size = snap_radius * 2.0;
        let result = snap_or_push_grid(Coord { x: 5.0, y: 5.0 }, &mut grid, snap_radius, cell_size);
        assert_eq!(result, Coord { x: 5.0, y: 5.0 });
        assert_eq!(grid.len(), 1);
    }

    #[test]
    fn test_snap_or_push_existing() {
        let mut grid = std::collections::HashMap::new();
        let snap_radius = 1e-8;
        let cell_size = snap_radius * 2.0;
        grid.insert(
            grid_key(Coord { x: 0.0, y: 0.0 }, cell_size),
            Coord { x: 0.0, y: 0.0 },
        );
        let result = snap_or_push_grid(
            Coord { x: 1e-10, y: 1e-10 },
            &mut grid,
            snap_radius,
            cell_size,
        );
        assert_eq!(result, Coord { x: 0.0, y: 0.0 });
        assert_eq!(grid.len(), 1);
    }

    #[test]
    fn test_snap_or_push_far() {
        let mut grid = std::collections::HashMap::new();
        let snap_radius = 1e-8;
        let cell_size = snap_radius * 2.0;
        let result = snap_or_push_grid(
            Coord { x: 0.01, y: 0.01 },
            &mut grid,
            snap_radius,
            cell_size,
        );
        assert_eq!(result, Coord { x: 0.01, y: 0.01 });
        assert_eq!(grid.len(), 1);
    }

    // -------------------------------
    // prepare_lines integration
    // -------------------------------

    #[test]
    fn test_prepare_lines_empty() {
        let result = prepare_lines(Vec::new());
        assert!(result.is_ok());
        let prepared = result.unwrap();
        assert!(prepared.lines.is_empty());
    }

    #[test]
    fn test_prepare_lines_single() {
        let lines = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        )];
        let result = prepare_lines(lines);
        assert!(result.is_ok());
        let prepared = result.unwrap();
        assert_eq!(prepared.lines.len(), 1);
    }

    #[test]
    fn test_prepare_lines_with_nan() {
        let lines = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: f64::NAN,
                y: 0.0,
            },
        )];
        let result = prepare_lines(lines);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MakeValidError::CoordinateIsNaN { .. }
        ));
    }

    #[test]
    fn test_split_segments_no_intersections() {
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }),
        ];
        let result = split_segments(lines.clone());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn test_split_segments_crossing() {
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 0.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
        ];
        let result = split_segments(lines);
        assert!(result.is_ok());
        let split = result.unwrap();
        // Should split each crossing segment into 2, plus snap
        assert!(split.len() >= 3);
    }

    #[test]
    fn diagnose_many_holes() {
        use geo::LinesIter;
        use std::time::Instant;

        let mut wkt = String::from("POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0)");
        for i in 0..10 {
            let x = 2.0 + (i as f64) * 8.0;
            let y = 2.0;
            wkt.push_str(&format!(
                ", ({x} {y}, {} {y}, {} {}, {x} {}, {x} {y})",
                x + 3.0,
                x + 3.0,
                y + 3.0,
                y + 3.0
            ));
        }
        wkt.push(')');

        let geo: geo::Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(&wkt).unwrap();
        let poly = match geo {
            geo::Geometry::Polygon(p) => p,
            _ => panic!("nope"),
        };
        println!(
            "diagnose: ext {} coords, {} holes",
            poly.exterior().0.len(),
            poly.interiors().len()
        );

        let lines: Vec<_> = poly.lines_iter().collect();
        println!("lines: {}", lines.len());

        // warmup
        for _ in 0..100 {
            let _ = has_no_intersections(&lines);
        }

        let start = Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(has_no_intersections(&lines));
        }
        let d = start.elapsed();
        println!("has_no_intersections x1000: {:?}, per: {:?}", d, d / 1000);

        let start = Instant::now();
        for _ in 0..1000 {
            let _ = std::hint::black_box(poly.clone());
        }
        let d = start.elapsed();
        println!("poly.clone x1000: {:?}, per: {:?}", d, d / 1000);

        let start = Instant::now();
        for _ in 0..1000 {
            let _ = std::hint::black_box(crate::arrange::poly_has_basic_form(&poly));
        }
        let d = start.elapsed();
        println!("poly_has_basic_form x1000: {:?}, per: {:?}", d, d / 1000);

        // Does has_no_intersections return true?
        let hni = has_no_intersections(&lines);
        println!("has_no_intersections returned: {hni}");

        let config = crate::config::MakeValidConfig {
            poly_method: crate::config::PolyMethod::Arrange,
            ..Default::default()
        };

        // full fix_polygon
        let start = Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(crate::arrange::fix_polygon(&poly, &config));
        }
        let d = start.elapsed();
        println!("fix_polygon x1000: {:?}, per: {:?}", d, d / 1000);
    }
}

use crate::orient::orient2d;
use geo::Line;
use rstar::{AABB, RTree, RTreeObject};

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
    ring_start: usize,
    ring_len: usize,
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

    for (i, line) in lines.iter().enumerate().skip(1) {
        // Force chain break at ring boundary
        let at_ring_boundary = i == ring_end;
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
            min_x = line.start.x.min(line.end.x);
            max_x = line.start.x.max(line.end.x);
            min_y = line.start.y.min(line.end.y);
            max_y = line.start.y.max(line.end.y);

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
        let o1 = orient2d(li.start, li.end, lj.start);
        let o2 = orient2d(li.start, li.end, lj.end);
        let o3 = orient2d(lj.start, lj.end, li.start);
        let o4 = orient2d(lj.start, lj.end, li.end);
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

    // Try fast grid path; fall back to R-tree if any cell gets too dense
    let grid_result = has_no_intersections_grid(&chains, lines);
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

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        let do_parallel = nc >= 200;
        if do_parallel {
            use rayon::prelude::*;
            use std::ops::ControlFlow;
            use std::sync::atomic::Ordering;
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

    use std::ops::ControlFlow;
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
/// too dense, triggering the R-tree fallback.
fn has_no_intersections_grid(chains: &[MonoChain], lines: &[Line<f64>]) -> Option<bool> {
    let nc = chains.len();
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for mc in chains {
        min_x = min_x.min(mc.min_x);
        max_x = max_x.max(mc.max_x);
        min_y = min_y.min(mc.min_y);
        max_y = max_y.max(mc.max_y);
    }
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

    for cell in &cell_chains {
        for ii in 0..cell.len() {
            let mc1 = &chains[cell[ii]];
            for jj in (ii + 1)..cell.len() {
                if compute_overlaps(lines, mc1, &chains[cell[jj]]) {
                    return Some(false);
                }
            }
        }
    }
    Some(true)
}

//! MultiLineString component repair: node the parts where they meet, keep every piece.
//!
//! OGC says the elements of a MultiCurve may meet only at points that are endpoints of both.
//! An input whose parts touch or cross is invalid, and the honest repair is to split the parts
//! at those points: the pieces then meet only at endpoints and the coverage is unchanged.
//! Only a collinear overlap cannot be split away, so it is deduplicated. The old behaviour
//! dropped every part that touched an earlier one.
use alloc::vec::Vec;

use geo::{Coord, Line};

use crate::noding::intersection::{compute_intersection_param, interpolate};
use crate::validation::impls::{check_line_components_intersect, segments_collinear_overlap};

type Run = Vec<Coord<f64>>;
type Cuts = Vec<Vec<(f64, Coord<f64>)>>;

/// Scale-relative snapping used by the ring and component checks.
fn scale_eps(parts: &[Run]) -> f64 {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in parts {
        for c in p {
            if c.x.is_finite() && c.y.is_finite() {
                min_x = min_x.min(c.x);
                max_x = max_x.max(c.x);
                min_y = min_y.min(c.y);
                max_y = max_y.max(c.y);
            }
        }
    }
    if min_x > max_x {
        return 1e-12;
    }
    let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    1e-12 * scale
}

/// Split every part at the points where it meets another part.
pub(crate) fn node_components(parts: &[Run]) -> Vec<Run> {
    if parts.len() < 2 {
        return parts.to_vec();
    }
    let eps = scale_eps(parts);
    let parts = split_closed_parts(parts, eps);
    let mut cuts: Vec<Cuts> =
        parts.iter().map(|p| vec![Vec::new(); p.len().saturating_sub(1)]).collect();

    for (a, b) in candidate_pairs(&parts, eps) {
        let (left, right) = cuts.split_at_mut(b);
        connect(&parts[a], &parts[b], &mut left[a], &mut right[0], eps);
    }

    let mut out: Vec<Run> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        out.extend(split_runs(part, &cuts[i], eps));
    }
    out = dedupe_covered(out, eps);

    // The validator's own rule is the final word: anything still conflicting is dropped.
    if any_conflict(&out, eps)
        && let Some(kept) = crate::make_valid::keep_conflicting_runs(&out)
    {
        out = kept.into_iter().map(|i| out[i].clone()).collect();
    }
    out
}

/// Min/max box of a run, or `None` when it has no coordinate.
fn run_bbox(run: &[Coord<f64>]) -> Option<(f64, f64, f64, f64)> {
    let mut bb = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for c in run {
        if !c.x.is_finite() || !c.y.is_finite() {
            return None;
        }
        bb.0 = bb.0.min(c.x);
        bb.1 = bb.1.max(c.x);
        bb.2 = bb.2.min(c.y);
        bb.3 = bb.3.max(c.y);
    }
    (bb.0 <= bb.1).then_some(bb)
}

/// Pairs `(i, j)` whose boxes come within eps, ordered by min x so the scan can stop early.
/// A pair further apart than eps cannot share a point or an overlap, so it is skipped.
fn candidate_pairs(runs: &[Run], eps: f64) -> Vec<(usize, usize)> {
    let boxes: Vec<Option<(f64, f64, f64, f64)>> = runs.iter().map(|r| run_bbox(r)).collect();
    let mut order: Vec<usize> = (0..runs.len()).filter(|&i| boxes[i].is_some()).collect();
    order.sort_by(|&a, &b| {
        boxes[a]
            .expect("filtered above")
            .0
            .partial_cmp(&boxes[b].expect("filtered above").0)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let mut out = Vec::new();
    for (k, &i) in order.iter().enumerate() {
        let (ax0, ax1, ay0, ay1) = boxes[i].expect("filtered above");
        for &j in &order[k + 1..] {
            let (bx0, bx1, by0, by1) = boxes[j].expect("filtered above");
            if bx0 > ax1 + eps {
                break;
            }
            if bx1 < ax0 - eps || by1 < ay0 - eps || by0 > ay1 + eps {
                continue;
            }
            out.push((i.min(j), i.max(j)));
        }
    }
    out
}

/// A closed component has no boundary, so it cannot legally touch anything. Split it into
/// two open paths at the contact point: both ends are boundaries then, and coverage stays.
fn split_closed_parts(parts: &[Run], eps: f64) -> Vec<Run> {
    let mut neigh: Vec<Vec<usize>> = vec![Vec::new(); parts.len()];
    for (i, j) in candidate_pairs(parts, eps) {
        neigh[i].push(j);
        neigh[j].push(i);
    }
    let mut out: Vec<Run> = Vec::with_capacity(parts.len());
    for (i, p) in parts.iter().enumerate() {
        let closed = p.len() > 3 && p[0] == p[p.len() - 1];
        let conflicts = closed
            && neigh[i]
                .iter()
                .any(|&j| check_line_components_intersect(p, &parts[j], eps));
        if !conflicts {
            out.push(p.clone());
            continue;
        }
        match split_closed(p, parts, i, eps) {
            Some((a_head, b_tail)) => {
                out.push(a_head);
                out.push(b_tail);
            }
            None => out.push(p.clone()),
        }
    }
    out
}

/// Rotate the closed ring to the first contact point and cut it into two open paths.
fn split_closed(part: &[Coord<f64>], parts: &[Run], self_idx: usize, eps: f64) -> Option<(Run, Run)> {
    let n = part.len() - 1;
    let contact = (0..n).find(|&k| {
        parts
            .iter()
            .enumerate()
            .any(|(j, q)| j != self_idx && vertex_touches(part[k], q, eps))
    })?;
    let second = (contact + 1) % n;
    if second == contact {
        return None;
    }
    let mut rotated: Run = Vec::with_capacity(n + 1);
    for t in 0..=n {
        rotated.push(part[(contact + t) % n]);
    }
    let cut = (second + n - contact) % n;
    if cut == 0 {
        return None;
    }
    let head: Run = rotated[..=cut].to_vec();
    let tail: Run = rotated[cut..].to_vec();
    if head.len() < 2 || tail.len() < 2 || head[0] == head[head.len() - 1] || tail[0] == tail[tail.len() - 1] {
        return None;
    }
    Some((head, tail))
}

fn vertex_touches(v: Coord<f64>, q: &[Coord<f64>], eps: f64) -> bool {
    if q.len() < 2 {
        return false;
    }
    q.iter().any(|c| (c.x - v.x).abs() <= eps && (c.y - v.y).abs() <= eps)
        || q.windows(2).any(|w| on_segment(w[0], w[1], v, eps))
}

/// Record a cut at parameter `t` of segment `seg`, keeping the exact point to insert.
fn add_cut(cuts: &mut [Vec<(f64, Coord<f64>)>], seg: usize, t: f64, point: Coord<f64>, eps: f64) {
    let Some(seg_cuts) = cuts.get_mut(seg) else { return };
    if t < 0.0 || t > 1.0 || !point.x.is_finite() || !point.y.is_finite() {
        return;
    }
    if seg_cuts.iter().any(|(s, p)| (*s - t).abs() <= eps && *p == point) {
        return;
    }
    seg_cuts.push((t, point));
}

/// Split `a` at its interior vertex `k` (0 < k < len-1).
fn add_vertex_cut(cuts: &mut [Vec<(f64, Coord<f64>)>], k: usize, point: Coord<f64>, eps: f64) {
    if k == 0 {
        return;
    }
    add_cut(cuts, k, 0.0, point, eps);
}

/// A cut at either segment end is a split at that vertex, not an interior point.
fn cut_at(cuts: &mut [Vec<(f64, Coord<f64>)>], seg: usize, t: f64, point: Coord<f64>, eps: f64) {
    if t < 0.0 || t > 1.0 {
        return;
    }
    if t <= eps {
        add_vertex_cut(cuts, seg, point, eps);
    } else if t >= 1.0 - eps {
        add_vertex_cut(cuts, seg + 1, point, eps);
    } else {
        add_cut(cuts, seg, t, point, eps);
    }
}

/// Cuts for one pair of parts: point contacts split both, collinear overlaps split at their ends.
fn connect(
    a: &[Coord<f64>],
    b: &[Coord<f64>],
    cuts_a: &mut [Vec<(f64, Coord<f64>)>],
    cuts_b: &mut [Vec<(f64, Coord<f64>)>],
    eps: f64,
) {
    if a.len() < 2 || b.len() < 2 {
        return;
    }
    for i in 0..a.len() - 1 {
        let ea = Line::new(a[i], a[i + 1]);
        for j in 0..b.len() - 1 {
            let eb = Line::new(b[j], b[j + 1]);
            if segments_collinear_overlap(a[i], a[i + 1], b[j], b[j + 1], eps) {
                // Both ends of the overlap are contacts, so each end must be a boundary of both parts.
                let t_bs = param_on(&ea, eb.start);
                let t_be = param_on(&ea, eb.end);
                let lo = 0.0f64.max(t_bs.min(t_be));
                let hi = 1.0f64.min(t_bs.max(t_be));
                for end in [lo, hi] {
                    let point = if (end - t_bs).abs() <= 1e-9 {
                        eb.start
                    } else if (end - t_be).abs() <= 1e-9 {
                        eb.end
                    } else {
                        interpolate(ea, end)
                    };
                    cut_at(cuts_a, i, end, point, eps);
                    cut_at(cuts_b, j, param_on(&eb, point), point, eps);
                }
                continue;
            }
            let Some((t, u, pt)) = compute_intersection_param(&ea, &eb, eps) else { continue };
            if t > eps && t < 1.0 - eps {
                add_cut(cuts_a, i, t, pt, eps);
            } else {
                let k = if t <= eps { i } else { i + 1 };
                add_vertex_cut(cuts_a, k, a[k.min(a.len() - 1)], eps);
            }
            if u > eps && u < 1.0 - eps {
                add_cut(cuts_b, j, u, pt, eps);
            } else {
                let k = if u <= eps { j } else { j + 1 };
                add_vertex_cut(cuts_b, k, b[k.min(b.len() - 1)], eps);
            }
        }
    }
}

/// Use the exact endpoint when the interpolated point is that endpoint within `eps`.
fn snap_to_endpoint(p: Coord<f64>, s: Coord<f64>, e: Coord<f64>, eps: f64) -> Coord<f64> {
    let near = |q: Coord<f64>| (p.x - q.x).abs() <= eps && (p.y - q.y).abs() <= eps;
    if near(s) {
        s
    } else if near(e) {
        e
    } else {
        p
    }
}

/// Param of `p` projected onto the edge.
fn param_on(e: &Line<f64>, p: Coord<f64>) -> f64 {
    let dx = e.end.x - e.start.x;
    let dy = e.end.y - e.start.y;
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return 0.0;
    }
    ((p.x - e.start.x) * dx + (p.y - e.start.y) * dy) / l2
}

/// Cut one part into runs at its recorded cut points; every cut becomes a component boundary.
fn split_runs(part: &[Coord<f64>], cuts: &[Vec<(f64, Coord<f64>)>], eps: f64) -> Vec<Run> {
    let n = part.len();
    if n < 2 {
        return Vec::new();
    }
    let mut out: Vec<Run> = Vec::new();
    let mut cur: Run = vec![part[0]];
    for i in 0..n - 1 {
        let mut segment_cuts: Vec<(f64, Coord<f64>)> =
            cuts.get(i).cloned().unwrap_or_default();
        segment_cuts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(core::cmp::Ordering::Equal));
        for (t, pt) in segment_cuts {
            if t <= eps {
                // A cut at the start of the segment: close the run at this vertex, start the next.
                if cur.len() >= 2 {
                    out.push(core::mem::take(&mut cur));
                }
                cur = vec![part[i]];
                continue;
            }
            if t >= 1.0 - eps {
                continue;
            }
            let point = snap_to_endpoint(pt, part[i], part[i + 1], eps);
            if cur.last() != Some(&point) {
                cur.push(point);
            }
            if cur.len() >= 2 {
                out.push(core::mem::take(&mut cur));
            }
            cur = vec![point];
        }
        let end = part[i + 1];
        if cur.last() != Some(&end) {
            cur.push(end);
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out.retain(|r| r.len() >= 2);
    out
}

/// Drop a piece that the other pieces already cover, which is a duplicated or overlapping line.
/// Drop every piece that the other pieces already cover, until no piece is redundant.
fn dedupe_covered(kept: Vec<Run>, eps: f64) -> Vec<Run> {
    let n = kept.len();
    let mut neigh: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, j) in candidate_pairs(&kept, eps) {
        neigh[i].push(j);
        neigh[j].push(i);
    }
    let mut alive = vec![true; n];
    let mut live = n;
    loop {
        let mut removed = false;
        for i in 0..n {
            if !alive[i] || live < 2 {
                continue;
            }
            if run_covered(&kept[i], &kept, &neigh[i], &alive, eps) {
                alive[i] = false;
                live -= 1;
                removed = true;
            }
        }
        if !removed {
            break;
        }
    }
    kept.into_iter()
        .zip(alive)
        .filter_map(|(run, keep)| keep.then_some(run))
        .collect()
}

/// Coverage tested only against the candidate pieces whose box comes within eps.
fn run_covered(
    run: &Run,
    kept: &[Run],
    neigh: &[usize],
    alive: &[bool],
    eps: f64,
) -> bool {
    run.windows(2).all(|w| {
        let mid = Coord {
            x: (w[0].x + w[1].x) / 2.0,
            y: (w[0].y + w[1].y) / 2.0,
        };
        [w[0], mid, w[1]].iter().all(|p| {
            neigh
                .iter()
                .any(|&j| alive[j] && point_on_run(&kept[j], *p, eps))
        })
    })
}

fn point_on_run(run: &Run, p: Coord<f64>, eps: f64) -> bool {
    run.windows(2).any(|s| on_segment(s[0], s[1], p, eps))
}

fn on_segment(s0: Coord<f64>, s1: Coord<f64>, p: Coord<f64>, eps: f64) -> bool {
    let dx = s1.x - s0.x;
    let dy = s1.y - s0.y;
    if dx == 0.0 && dy == 0.0 {
        return (p.x - s0.x).abs() <= eps && (p.y - s0.y).abs() <= eps;
    }
    // Absolute tolerance, the same reach as the component check: eps is a length.
    let cross = dx * (p.y - s0.y) - dy * (p.x - s0.x);
    if cross.abs() > eps * (dx * dx + dy * dy).sqrt() {
        return false;
    }
    p.x >= s0.x.min(s1.x) - eps
        && p.x <= s0.x.max(s1.x) + eps
        && p.y >= s0.y.min(s1.y) - eps
        && p.y <= s0.y.max(s1.y) + eps
}

fn any_conflict(parts: &[Run], eps: f64) -> bool {
    candidate_pairs(parts, eps)
        .into_iter()
        .any(|(i, j)| check_line_components_intersect(&parts[i], &parts[j], eps))
}



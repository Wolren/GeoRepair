//! Full Bentley-Ottmann sweep-line for finding all proper edge intersections.
//!
//! Processes endpoint events left-to-right, maintains an active set of
//! segments intersecting the current sweep position, and detects intersections
//! between adjacent segments.  When an intersection is found it is added as a
//! future event so that crossing segments are swapped in the active set,
//! ensuring all subsequent intersections are found.

use rustc_hash::FxHashSet;

use geo::Line;

use crate::orient::orient2d;

// ── Events ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum EventKind {
    Left,
    Right,
    Crossing,
}

#[derive(Debug, Clone)]
struct Event {
    x: f64,
    y: f64,
    kind: EventKind,
    edge: usize,
    other: usize,
}

// ── Active set ──────────────────────────────────────────────────────────

struct ActiveSet<'a> {
    edges: &'a [Line<f64>],
    order: Vec<usize>,
    sweep_x: f64,
}

impl<'a> ActiveSet<'a> {
    fn new(edges: &'a [Line<f64>]) -> Self {
        Self {
            edges,
            order: Vec::new(),
            sweep_x: f64::NEG_INFINITY,
        }
    }

    fn set_sweep_x(&mut self, x: f64) {
        self.sweep_x = x;
    }

    fn y_at_x(&self, idx: usize) -> f64 {
        let e = &self.edges[idx];
        let dx = e.end.x - e.start.x;
        if dx.abs() <= f64::EPSILON * 100.0 {
            return (e.start.y + e.end.y) * 0.5;
        }
        let t = (self.sweep_x - e.start.x) / dx;
        e.start.y + t * (e.end.y - e.start.y)
    }

    fn insert(&mut self, idx: usize) {
        let y = self.y_at_x(idx);
        let pos = self
            .order
            .binary_search_by(|&oi| {
                self.y_at_x(oi)
                    .partial_cmp(&y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or_else(|e| e);
        self.order.insert(pos, idx);
    }

    fn remove(&mut self, idx: usize) -> usize {
        let pos = self.order.iter().position(|&oi| oi == idx).unwrap();
        self.order.remove(pos);
        pos
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.order.swap(i, j);
    }

    fn len(&self) -> usize {
        self.order.len()
    }

    fn neighbors(&self, pos: usize) -> (Option<usize>, Option<usize>) {
        let prev = if pos > 0 {
            Some(self.order[pos - 1])
        } else {
            None
        };
        let next = if pos + 1 < self.order.len() {
            Some(self.order[pos + 1])
        } else {
            None
        };
        (prev, next)
    }

    fn position_of(&self, idx: usize) -> Option<usize> {
        self.order.iter().position(|&oi| oi == idx)
    }
}

// ── Crossing detection ──────────────────────────────────────────────────

fn edges_cross(e1: &Line<f64>, e2: &Line<f64>, _eps: f64) -> bool {
    if e1.start == e2.start || e1.start == e2.end || e1.end == e2.start || e1.end == e2.end {
        return false;
    }
    let o1 = orient2d(e1.start, e1.end, e2.start);
    let o2 = orient2d(e1.start, e1.end, e2.end);
    if o1.signum() == o2.signum() && o1 != 0.0 && o2 != 0.0 {
        return false;
    }
    let o3 = orient2d(e2.start, e2.end, e1.start);
    let o4 = orient2d(e2.start, e2.end, e1.end);
    if o3.signum() == o4.signum() && o3 != 0.0 && o4 != 0.0 {
        return false;
    }
    if o1 == 0.0 && o2 == 0.0 && o3 == 0.0 && o4 == 0.0 {
        return false;
    }
    true
}

fn edge_intersection_xy(e1: &Line<f64>, e2: &Line<f64>) -> Option<(f64, f64)> {
    let dx1 = e1.end.x - e1.start.x;
    let dy1 = e1.end.y - e1.start.y;
    let dx2 = e2.end.x - e2.start.x;
    let dy2 = e2.end.y - e2.start.y;
    let denom = dx1 * dy2 - dy1 * dx2;
    if denom.abs() <= f64::EPSILON * 100.0 {
        return None;
    }
    let dx3 = e2.start.x - e1.start.x;
    let dy3 = e2.start.y - e1.start.y;
    let t = (dx3 * dy2 - dy3 * dx2) / denom;
    let u = (dx3 * dy1 - dy3 * dx1) / denom;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Some((e1.start.x + t * dx1, e1.start.y + t * dy1))
    } else {
        None
    }
}

// ── Event ordering ──────────────────────────────────────────────────────

fn event_cmp(a: &Event, b: &Event) -> std::cmp::Ordering {
    a.x.partial_cmp(&b.x)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
        .then_with(|| {
            let ak: u8 = match a.kind {
                EventKind::Left => 0,
                EventKind::Crossing => 1,
                EventKind::Right => 2,
            };
            let bk: u8 = match b.kind {
                EventKind::Left => 0,
                EventKind::Crossing => 1,
                EventKind::Right => 2,
            };
            ak.cmp(&bk)
        })
}

#[inline]
fn pair_key(i: usize, j: usize) -> u64 {
    if i < j {
        (i as u64) << 32 | j as u64
    } else {
        (j as u64) << 32 | i as u64
    }
}

// ── Main sweep ──────────────────────────────────────────────────────────

pub(crate) fn find_intersecting_pairs(edges: &[Line<f64>], eps: f64) -> Vec<(usize, usize)> {
    let n = edges.len();
    if n < 2 {
        return vec![];
    }

    // 1. Build endpoint events
    let mut events: Vec<Event> = Vec::with_capacity(n * 2);
    for (i, e) in edges.iter().enumerate() {
        let (left_pt, right_pt) =
            if e.start.x < e.end.x || (e.start.x == e.end.x && e.start.y < e.end.y) {
                (e.start, e.end)
            } else {
                (e.end, e.start)
            };
        events.push(Event {
            x: left_pt.x,
            y: left_pt.y,
            kind: EventKind::Left,
            edge: i,
            other: 0,
        });
        events.push(Event {
            x: right_pt.x,
            y: right_pt.y,
            kind: EventKind::Right,
            edge: i,
            other: 0,
        });
    }

    events.sort_by(event_cmp);

    // 2. Sweep — use a separate pending list for crossing events to avoid
    //    borrow conflicts with the main event list.
    let mut active = ActiveSet::new(edges);
    let mut results: Vec<(usize, usize)> = Vec::new();
    let mut seen: FxHashSet<u64> = FxHashSet::default();
    let mut pending: Vec<Event> = Vec::new();
    let mut event_idx = 0;

    while event_idx < events.len() || !pending.is_empty() {
        let ev = if event_idx < events.len() {
            if !pending.is_empty()
                && event_cmp(&pending[0], &events[event_idx]) == std::cmp::Ordering::Less
            {
                pending.remove(0)
            } else {
                let ev = events[event_idx].clone();
                event_idx += 1;
                ev
            }
        } else {
            pending.remove(0)
        };

        active.set_sweep_x(ev.x);

        match ev.kind {
            EventKind::Left => {
                active.insert(ev.edge);
                let pos = active.position_of(ev.edge).unwrap();
                if let Some(p) = active.neighbors(pos).0 {
                    check_and_schedule(
                        edges,
                        eps,
                        p,
                        ev.edge,
                        ev.x,
                        &mut results,
                        &mut seen,
                        &mut pending,
                    );
                }
                if let Some(n) = active.neighbors(pos).1 {
                    check_and_schedule(
                        edges,
                        eps,
                        ev.edge,
                        n,
                        ev.x,
                        &mut results,
                        &mut seen,
                        &mut pending,
                    );
                }
            }
            EventKind::Right => {
                let pos = active.remove(ev.edge);
                if pos > 0 && pos < active.len() {
                    let i = active.order[pos - 1];
                    let j = active.order[pos];
                    check_and_schedule(
                        edges,
                        eps,
                        i,
                        j,
                        ev.x,
                        &mut results,
                        &mut seen,
                        &mut pending,
                    );
                }
            }
            EventKind::Crossing => {
                let pos_i = active.position_of(ev.edge);
                let pos_j = active.position_of(ev.other);
                if let (Some(pi), Some(pj)) = (pos_i, pos_j) {
                    let (lo, hi) = if pi < pj { (pi, pj) } else { (pj, pi) };
                    if hi != lo + 1 {
                        continue;
                    }
                    active.swap(lo, hi);
                    if lo > 0 {
                        let a = active.order[lo - 1];
                        let b = active.order[lo];
                        check_and_schedule(
                            edges,
                            eps,
                            a,
                            b,
                            ev.x,
                            &mut results,
                            &mut seen,
                            &mut pending,
                        );
                    }
                    if hi + 1 < active.len() {
                        let a = active.order[hi];
                        let b = active.order[hi + 1];
                        check_and_schedule(
                            edges,
                            eps,
                            a,
                            b,
                            ev.x,
                            &mut results,
                            &mut seen,
                            &mut pending,
                        );
                    }
                }
            }
        }
    }

    results
}

#[allow(clippy::too_many_arguments)]
fn check_and_schedule(
    edges: &[Line<f64>],
    eps: f64,
    i: usize,
    j: usize,
    cur_x: f64,
    results: &mut Vec<(usize, usize)>,
    seen: &mut FxHashSet<u64>,
    pending: &mut Vec<Event>,
) {
    let key = pair_key(i, j);
    if seen.contains(&key) {
        return;
    }
    if !edges_cross(&edges[i], &edges[j], eps) {
        seen.insert(key);
        return;
    }
    seen.insert(key);
    results.push((i, j));

    if let Some((ix, iy)) = edge_intersection_xy(&edges[i], &edges[j])
        && ix > cur_x + f64::EPSILON * 200.0
    {
        let ev = Event {
            x: ix,
            y: iy,
            kind: EventKind::Crossing,
            edge: i,
            other: j,
        };
        let pos = pending
            .binary_search_by(|pev| event_cmp(pev, &ev))
            .unwrap_or_else(|e| e);
        pending.insert(pos, ev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Coord, Line};

    fn l(x1: f64, y1: f64, x2: f64, y2: f64) -> Line<f64> {
        Line::new(Coord { x: x1, y: y1 }, Coord { x: x2, y: y2 })
    }

    #[test]
    fn test_no_intersections() {
        let edges = vec![l(0.0, 0.0, 1.0, 0.0), l(2.0, 0.0, 3.0, 0.0)];
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_single_cross() {
        let edges = vec![l(0.0, 0.0, 2.0, 2.0), l(0.0, 2.0, 2.0, 0.0)];
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_adjacent_edges_no_cross() {
        let edges = vec![l(0.0, 0.0, 1.0, 0.0), l(1.0, 0.0, 2.0, 0.0)];
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_shared_endpoint_no_cross() {
        let edges = vec![l(0.0, 0.0, 1.0, 0.0), l(0.0, 0.0, 0.0, 1.0)];
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_bowtie_diagonals() {
        let edges = vec![l(0.0, 0.0, 10.0, 10.0), l(10.0, 0.0, 0.0, 10.0)];
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_three_edges_multiple_crossings() {
        let edges = vec![
            l(0.0, 0.0, 10.0, 10.0),
            l(0.0, 5.0, 10.0, 5.0),
            l(0.0, 10.0, 10.0, 0.0),
        ];
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_spoke_wheel_no_cross() {
        // Spoke wheel segments: origin(0,0) to rim and back
        let n = 10;
        let mut edges = Vec::new();
        for i in 0..n {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            let rim = Coord {
                x: a.cos(),
                y: a.sin(),
            };
            edges.push(Line::new(Coord { x: 0.0, y: 0.0 }, rim));
            edges.push(Line::new(rim, Coord { x: 0.0, y: 0.0 }));
        }
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_collinear_no_cross() {
        let edges = vec![l(0.0, 0.0, 10.0, 0.0), l(5.0, 0.0, 15.0, 0.0)];
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_vertical_edge() {
        let edges = vec![l(5.0, 0.0, 5.0, 10.0), l(0.0, 5.0, 10.0, 5.0)];
        let result = find_intersecting_pairs(&edges, 1e-10);
        assert_eq!(result.len(), 1);
    }
}

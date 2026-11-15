//! Planar faces of a noded edge set.
//!
//! Duplicate segments cancel by parity; dangles and bridges are pruned. Even-odd
//! membership is the dual walk parity from the unbounded face.

use ::core::ops::ControlFlow;
use alloc::vec::Vec;
use geo::{Coord, Line, LineString, MultiPolygon, Polygon};
use rstar::{AABB, RTree, RTreeObject};
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::arrange::prep::prepare_lines;
use crate::orient::orient2d;
use crate::structure::fix_ring_graph::{
    Graph, extract_all_faces_polygonize, split_face_at_pinch_points,
};
use crate::structure::symdiff::segment_key;

/// Unbounded-face marker shared by the cycle ownership and parity walks.
const NONE: usize = usize::MAX;

/// One simple boundary ring, stored in walk order without the closing point.
struct Cycle {
    coords: Vec<Coord<f64>>,
    /// Twice the signed area: negative for a shell walk, positive for a hole walk.
    area2: f64,
    bbox: AABB<[f64; 2]>,
    /// Index of the cycle on the other side of the first half-edge.
    opposite: usize,
    /// First half-edge id, used to find the opposite cycle after the walk.
    first_half: usize,
}

/// R-tree entry that maps a query hit back to its cycle index.
#[derive(Clone, Copy)]
struct CycleEnv {
    idx: usize,
    env: AABB<[f64; 2]>,
}

impl RTreeObject for CycleEnv {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

#[inline]
fn canonical(c: Coord<f64>) -> Coord<f64> {
    Coord {
        x: if c.x == 0.0 { 0.0 } else { c.x },
        y: if c.y == 0.0 { 0.0 } else { c.y },
    }
}

#[inline]
fn vkey(c: Coord<f64>) -> (u64, u64) {
    let c = canonical(c);
    (c.x.to_bits(), c.y.to_bits())
}

#[inline]
fn finite(c: Coord<f64>) -> bool {
    c.x.is_finite() && c.y.is_finite()
}

fn ring_bbox(coords: &[Coord<f64>]) -> AABB<[f64; 2]> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for c in coords {
        min_x = min_x.min(c.x);
        min_y = min_y.min(c.y);
        max_x = max_x.max(c.x);
        max_y = max_y.max(c.y);
    }
    AABB::from_corners([min_x, min_y], [max_x, max_y])
}

fn signed_area2(coords: &[Coord<f64>]) -> f64 {
    let n = coords.len();
    let mut sum = 0.0;
    for i in 0..n {
        let a = coords[i];
        let b = coords[(i + 1) % n];
        sum += a.x * b.y - b.x * a.y;
    }
    sum
}

/// Cancels duplicate undirected segments by parity, keeping first-occurrence order.
fn parity_filter(lines: &[Line<f64>]) -> Vec<Line<f64>> {
    let mut slot_of: FxHashMap<(u64, u64, u64, u64), usize> = FxHashMap::default();
    let mut slots: Vec<Line<f64>> = Vec::new();
    let mut odd: Vec<bool> = Vec::new();
    for line in lines {
        let a = canonical(line.start);
        let b = canonical(line.end);
        if !finite(a) || !finite(b) || a == b {
            continue;
        }
        let key = segment_key(a, b);
        if let Some(&slot) = slot_of.get(&key) {
            odd[slot] = !odd[slot];
        } else {
            slot_of.insert(key, slots.len());
            slots.push(Line::new(a, b));
            odd.push(true);
        }
    }
    slots
        .into_iter()
        .zip(odd)
        .filter_map(|(line, keep)| keep.then_some(line))
        .collect()
}

/// Joins collinear chains through their interior degree-2 vertices.
fn merge_collinear(lines: Vec<Line<f64>>) -> Vec<Line<f64>> {
    let n = lines.len();
    if n < 2 {
        return lines;
    }
    let mut incident: FxHashMap<(u64, u64), SmallVec<[u32; 4]>> = FxHashMap::default();
    for (i, line) in lines.iter().enumerate() {
        incident.entry(vkey(line.start)).or_default().push(i as u32);
        incident.entry(vkey(line.end)).or_default().push(i as u32);
    }
    let far = |line: &Line<f64>, v: (u64, u64)| {
        if vkey(line.start) == v {
            line.end
        } else {
            line.start
        }
    };
    let joins = |v: (u64, u64), from: usize, used: &[bool]| -> Option<usize> {
        let inc = incident.get(&v)?;
        if inc.len() != 2 {
            return None;
        }
        let other = if inc[0] as usize == from {
            inc[1]
        } else {
            inc[0]
        } as usize;
        if other == from || used[other] {
            return None;
        }
        let (vp, op) = (far(&lines[from], v), far(&lines[other], v));
        if vkey(vp) == vkey(op) || vkey(vp) == v || vkey(op) == v {
            return None;
        }
        let vv = Coord {
            x: f64::from_bits(v.0),
            y: f64::from_bits(v.1),
        };
        let dx = op.x - vp.x;
        let dy = op.y - vp.y;
        let len2 = dx * dx + dy * dy;
        let cross = (vv.x - vp.x) * dy - (vv.y - vp.y) * dx;
        let scale = 1.0 + vp.x.abs().max(vp.y.abs()).max(op.x.abs()).max(op.y.abs());
        let eps = 1e-12 * scale;
        if cross * cross > eps * eps * len2 {
            return None;
        }
        let straight = (vv.x - vp.x) * (op.x - vv.x) + (vv.y - vp.y) * (op.y - vv.y) > 0.0;
        straight.then_some(other)
    };
    let mut used = vec![false; n];
    let mut out: Vec<Line<f64>> = Vec::with_capacity(n);
    for i in 0..n {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut head = lines[i].start;
        let mut tail = lines[i].end;
        let mut cur = i;
        while let Some(next) = joins(vkey(tail), cur, &used) {
            used[next] = true;
            tail = far(&lines[next], vkey(tail));
            cur = next;
        }
        cur = i;
        while let Some(prev) = joins(vkey(head), cur, &used) {
            used[prev] = true;
            head = far(&lines[prev], vkey(head));
            cur = prev;
        }
        if vkey(head) != vkey(tail) {
            out.push(Line::new(head, tail));
        }
    }
    out
}

/// Index of a coordinate, keyed by exact bits after the -0.0 fold.
fn vertex_index(
    c: Coord<f64>,
    map: &mut FxHashMap<(u64, u64), usize>,
    verts: &mut Vec<Coord<f64>>,
) -> usize {
    let key = (c.x.to_bits(), c.y.to_bits());
    if let Some(&idx) = map.get(&key) {
        return idx;
    }
    let idx = verts.len();
    verts.push(c);
    map.insert(key, idx);
    idx
}

/// Removes trees and isolated chains: an edge with a degree-1 end bounds no face.
fn prune_dangles(edges: &mut Vec<(usize, usize)>, nv: usize) {
    if edges.is_empty() {
        return;
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nv];
    for (e, &(u, v)) in edges.iter().enumerate() {
        adj[u].push(e);
        adj[v].push(e);
    }
    let mut degree: Vec<usize> = adj.iter().map(Vec::len).collect();
    let mut alive = vec![true; edges.len()];
    let mut stack: Vec<usize> = (0..nv).filter(|&v| degree[v] == 1).collect();
    while let Some(v) = stack.pop() {
        if degree[v] != 1 {
            continue;
        }
        let Some(&e) = adj[v].iter().find(|&&e| alive[e]) else {
            continue;
        };
        alive[e] = false;
        let (u, w) = edges[e];
        for x in [u, w] {
            degree[x] -= 1;
            if degree[x] == 1 {
                stack.push(x);
            }
        }
    }
    let mut i = 0;
    edges.retain(|_| {
        let keep = alive[i];
        i += 1;
        keep
    });
}

/// Removes bridges: an edge whose two half-edges bound one face bounds none.
fn prune_bridges(edges: &mut Vec<(usize, usize)>, nv: usize) {
    let ne = edges.len();
    if ne == 0 {
        return;
    }
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nv];
    for (e, &(u, v)) in edges.iter().enumerate() {
        adj[u].push((v, e));
        adj[v].push((u, e));
    }
    let mut disc = vec![NONE; nv];
    let mut low = vec![0usize; nv];
    let mut parent_edge = vec![NONE; nv];
    let mut bridge = vec![false; ne];
    let mut timer = 0usize;
    for root in 0..nv {
        if disc[root] != NONE || adj[root].is_empty() {
            continue;
        }
        disc[root] = timer;
        low[root] = timer;
        timer += 1;
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(v, pos)) = stack.last() {
            if pos < adj[v].len() {
                stack.last_mut().expect("non-empty stack").1 += 1;
                let (to, e) = adj[v][pos];
                if e == parent_edge[v] {
                    continue;
                }
                if disc[to] == NONE {
                    disc[to] = timer;
                    low[to] = timer;
                    timer += 1;
                    parent_edge[to] = e;
                    stack.push((to, 0));
                } else {
                    low[v] = low[v].min(disc[to]);
                }
            } else {
                stack.pop();
                if let Some(&(parent, _)) = stack.last() {
                    low[parent] = low[parent].min(low[v]);
                    if low[v] > disc[parent] {
                        bridge[parent_edge[v]] = true;
                    }
                }
            }
        }
    }
    let mut i = 0;
    edges.retain(|_| {
        let keep = !bridge[i];
        i += 1;
        keep
    });
}

/// Adjacency lists sorted counter-clockwise with a coordinate tie-break.
fn sorted_adjacency(
    verts: &[Coord<f64>],
    edges: &[(usize, usize)],
) -> Vec<SmallVec<[(usize, usize); 4]>> {
    let nv = verts.len();
    let mut adj: Vec<Vec<(f64, usize, usize)>> = vec![Vec::new(); nv];
    for (e, &(u, v)) in edges.iter().enumerate() {
        let a = verts[u];
        let b = verts[v];
        adj[u].push(((b.y - a.y).atan2(b.x - a.x), v, e));
        adj[v].push(((a.y - b.y).atan2(a.x - b.x), u, e));
    }
    adj.into_iter()
        .map(|mut list| {
            list.sort_by(|l, r| {
                l.0.total_cmp(&r.0)
                    .then_with(|| verts[l.1].x.total_cmp(&verts[r.1].x))
                    .then_with(|| verts[l.1].y.total_cmp(&verts[r.1].y))
                    .then_with(|| l.2.cmp(&r.2))
            });
            list.into_iter().map(|(_, n, e)| (n, e)).collect()
        })
        .collect()
}

fn build_graph(lines: &[Line<f64>]) -> Graph {
    let mut key_to_idx: FxHashMap<(u64, u64), usize> = FxHashMap::default();
    let mut verts: Vec<Coord<f64>> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(lines.len());
    for line in lines {
        let a = canonical(line.start);
        let b = canonical(line.end);
        if a == b {
            continue;
        }
        let ia = vertex_index(a, &mut key_to_idx, &mut verts);
        let ib = vertex_index(b, &mut key_to_idx, &mut verts);
        if ia != ib {
            edges.push((ia, ib));
        }
    }
    prune_dangles(&mut edges, verts.len());
    prune_bridges(&mut edges, verts.len());
    let sorted_adj = sorted_adjacency(&verts, &edges);
    Graph {
        verts,
        edges,
        sorted_adj,
    }
}

fn ring_is_simple(ids: &[usize]) -> bool {
    let mut seen: FxHashSet<usize> = FxHashSet::default();
    ids.iter().all(|&id| seen.insert(id))
}

/// Extracts every minimal face boundary, with the half-edge owner of each side.
fn extract_cycles(graph: &Graph) -> (Vec<Cycle>, Vec<usize>) {
    let ne = graph.edges.len();
    let mut owner = vec![NONE; ne * 2];
    let mut cycles: Vec<Cycle> = Vec::new();
    let Some(faces) = extract_all_faces_polygonize(graph) else {
        return (cycles, owner);
    };
    for face in faces {
        for sub in split_face_at_pinch_points(&face, &graph.edges) {
            if sub.len() < 3 {
                continue;
            }
            let (first_edge, first_head) = sub[0];
            let (from, to) = graph.edges[first_edge];
            let start = if to == first_head { from } else { to };
            let mut ids: Vec<usize> = Vec::with_capacity(sub.len() + 1);
            ids.push(start);
            let mut half_edges: Vec<usize> = Vec::with_capacity(sub.len());
            for &(e, head) in &sub {
                ids.push(head);
                half_edges.push(if graph.edges[e].1 == head { e } else { e + ne });
            }
            if ids.len() < 4 || ids[ids.len() - 1] != ids[0] {
                continue;
            }
            ids.pop();
            if !ring_is_simple(&ids) {
                continue;
            }
            let coords: Vec<Coord<f64>> = ids.iter().map(|&id| graph.verts[id]).collect();
            let area2 = signed_area2(&coords);
            if area2 == 0.0 {
                continue;
            }
            let idx = cycles.len();
            let first_half = half_edges[0];
            for h in half_edges {
                if owner[h] == NONE {
                    owner[h] = idx;
                }
            }
            cycles.push(Cycle {
                bbox: ring_bbox(&coords),
                coords,
                area2,
                opposite: NONE,
                first_half,
            });
        }
    }
    for cycle in cycles.iter_mut() {
        let h = cycle.first_half;
        let twin = if h < ne { h + ne } else { h - ne };
        cycle.opposite = owner[twin];
    }
    (cycles, owner)
}

#[inline]
fn on_segment(a: Coord<f64>, b: Coord<f64>, p: Coord<f64>) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// 1 inside, 0 outside, -1 on the boundary.
fn locate_point(pt: Coord<f64>, ring: &[Coord<f64>]) -> i8 {
    let n = ring.len();
    let mut inside = false;
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        if orient2d(a, b, pt) == 0.0 && on_segment(a, b, pt) {
            return -1;
        }
        if (a.y > pt.y) != (b.y > pt.y) {
            let x = a.x + (pt.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if pt.x < x {
                inside = !inside;
            }
        }
    }
    if inside { 1 } else { 0 }
}

/// True when a non-boundary vertex of `inner` lies inside `outer`.
fn ring_inside(inner: &Cycle, outer: &Cycle) -> bool {
    for &p in &inner.coords {
        match locate_point(p, &outer.coords) {
            1 => return true,
            0 => return false,
            _ => {}
        }
    }
    false
}

/// Assigns every hole walk to the smallest shell walk that contains it.
fn assign_holes(cycles: &[Cycle]) -> Vec<Vec<usize>> {
    let mut holes: Vec<Vec<usize>> = vec![Vec::new(); cycles.len()];
    let shells: Vec<CycleEnv> = cycles
        .iter()
        .enumerate()
        .filter(|(_, c)| c.area2 < 0.0)
        .map(|(idx, c)| CycleEnv { idx, env: c.bbox })
        .collect();
    if shells.is_empty() {
        return holes;
    }
    let tree = RTree::bulk_load(shells);
    for (h, cycle) in cycles.iter().enumerate() {
        if cycle.area2 < 0.0 {
            continue;
        }
        let mut candidates: Vec<usize> = Vec::new();
        let _ = tree.locate_in_envelope_intersecting_int(cycle.bbox, |c| {
            candidates.push(c.idx);
            ControlFlow::<(), ()>::Continue(())
        });
        candidates.sort_by(|&a, &b| cycles[b].area2.total_cmp(&cycles[a].area2));
        for shell in candidates {
            // The twin walk bounds the same ring from the other side.
            if shell == cycle.opposite {
                continue;
            }
            // A hole ring is strictly smaller than the shell that contains it.
            if cycles[shell].area2.abs() <= cycle.area2.abs() {
                continue;
            }
            if ring_inside(cycle, &cycles[shell]) {
                holes[shell].push(h);
                break;
            }
        }
    }
    holes
}

/// Maps every cycle to its face node; the unbounded face is the last node.
fn face_nodes(cycles: &[Cycle], holes: &[Vec<usize>]) -> (Vec<usize>, usize) {
    let unbounded = cycles.iter().filter(|c| c.area2 < 0.0).count();
    let mut node = vec![unbounded; cycles.len()];
    let mut next = 0;
    for (i, cycle) in cycles.iter().enumerate() {
        if cycle.area2 < 0.0 {
            node[i] = next;
            next += 1;
        }
    }
    for (shell, list) in holes.iter().enumerate() {
        for &hole in list {
            node[hole] = node[shell];
        }
    }
    (node, unbounded)
}

/// Even-odd parity per face, walked from the unbounded face across every edge.
fn face_parity(owner: &[usize], ne: usize, node: &[usize], unbounded: usize) -> Vec<bool> {
    let mut parity: Vec<Option<bool>> = vec![None; unbounded + 1];
    parity[unbounded] = Some(false);
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); unbounded + 1];
    for e in 0..ne {
        let a = owner[e];
        let b = owner[e + ne];
        if a == NONE || b == NONE || a == b {
            continue;
        }
        let (fa, fb) = (node[a], node[b]);
        if fa == fb {
            continue;
        }
        adj[fa].push(fb);
        adj[fb].push(fa);
    }
    let mut stack: Vec<usize> = vec![unbounded];
    while let Some(f) = stack.pop() {
        let p = parity[f].unwrap_or(false);
        for &nb in &adj[f] {
            if parity[nb].is_none() {
                parity[nb] = Some(!p);
                stack.push(nb);
            }
        }
    }
    parity.into_iter().map(|p| p.unwrap_or(true)).collect()
}

/// Builds polygons from shell walks and their hole walks, plus the boundary lines.
fn assemble(
    cycles: &[Cycle],
    holes: &[Vec<usize>],
    parity: &[bool],
    node: &[usize],
    even_odd_only: bool,
) -> (Vec<Polygon<f64>>, Vec<Line<f64>>) {
    let mut polygons: Vec<Polygon<f64>> = Vec::new();
    let mut boundary: Vec<Line<f64>> = Vec::new();
    for (i, cycle) in cycles.iter().enumerate() {
        if cycle.area2 >= 0.0 || (even_odd_only && !parity[node[i]]) {
            continue;
        }
        let mut ring: Vec<Coord<f64>> = cycle.coords.iter().rev().copied().collect();
        ring.push(ring[0]);
        let exterior = LineString::new(ring);
        boundary.extend(exterior.0.windows(2).map(|w| Line::new(w[0], w[1])));
        let mut interiors: Vec<LineString<f64>> = Vec::with_capacity(holes[i].len());
        for &h in &holes[i] {
            let mut ring: Vec<Coord<f64>> = cycles[h].coords.iter().rev().copied().collect();
            ring.push(ring[0]);
            let hole = LineString::new(ring);
            boundary.extend(hole.0.windows(2).map(|w| Line::new(w[0], w[1])));
            interiors.push(hole);
        }
        polygons.push(Polygon::new(exterior, interiors));
    }
    (polygons, boundary)
}

fn subdivision(lines: &[Line<f64>]) -> Option<(Vec<Cycle>, Vec<usize>, usize)> {
    let prepared = prepare_lines(lines.to_vec()).ok()?;
    let unique = merge_collinear(parity_filter(&prepared.lines));
    let graph = build_graph(&unique);
    let ne = graph.edges.len();
    let (cycles, owner) = extract_cycles(&graph);
    Some((cycles, owner, ne))
}

fn subdivision_noded(lines: &[Line<f64>]) -> Option<(Vec<Cycle>, Vec<usize>, usize)> {
    let unique = merge_collinear(parity_filter(lines));
    let graph = build_graph(&unique);
    let ne = graph.edges.len();
    let (cycles, owner) = extract_cycles(&graph);
    Some((cycles, owner, ne))
}

fn assemble_area(
    cycles: Vec<Cycle>,
    owner: Vec<usize>,
    ne: usize,
) -> (MultiPolygon<f64>, Vec<Line<f64>>) {
    let holes = assign_holes(&cycles);
    let (node, unbounded) = face_nodes(&cycles, &holes);
    let parity = face_parity(&owner, ne, &node, unbounded);
    let (polygons, boundary) = assemble(&cycles, &holes, &parity, &node, true);
    (MultiPolygon::new(polygons), boundary)
}

/// Even-odd interior of the edge set, with the boundary lines of the built area.
pub(crate) fn even_odd_area(lines: &[Line<f64>]) -> Option<(MultiPolygon<f64>, Vec<Line<f64>>)> {
    let (cycles, owner, ne) = subdivision(lines)?;
    Some(assemble_area(cycles, owner, ne))
}

/// Even-odd interior of an edge set that the caller already welded and split.
pub(crate) fn even_odd_area_noded(
    lines: &[Line<f64>],
) -> Option<(MultiPolygon<f64>, Vec<Line<f64>>)> {
    let (cycles, owner, ne) = subdivision_noded(lines)?;
    Some(assemble_area(cycles, owner, ne))
}

/// Every bounded face of the edge set, holes assigned.
pub fn polygonize(lines: &[Line<f64>]) -> Vec<Polygon<f64>> {
    let Some((cycles, owner, ne)) = subdivision(lines) else {
        return Vec::new();
    };
    let holes = assign_holes(&cycles);
    let (node, unbounded) = face_nodes(&cycles, &holes);
    let parity = face_parity(&owner, ne, &node, unbounded);
    assemble(&cycles, &holes, &parity, &node, false).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Area, Coord, Line};

    fn ring(coords: &[(f64, f64)]) -> Vec<Line<f64>> {
        coords
            .windows(2)
            .map(|w| {
                Line::new(
                    Coord {
                        x: w[0].0,
                        y: w[0].1,
                    },
                    Coord {
                        x: w[1].0,
                        y: w[1].1,
                    },
                )
            })
            .collect()
    }

    fn square(x: f64, y: f64, width: f64) -> Vec<Line<f64>> {
        ring(&[
            (x, y),
            (x + width, y),
            (x + width, y + width),
            (x, y + width),
            (x, y),
        ])
    }

    fn total_area(polys: &[Polygon<f64>]) -> f64 {
        polys.iter().map(Area::unsigned_area).sum()
    }

    #[test]
    fn polygonize_keeps_one_face_per_bounded_region() {
        let faces = polygonize(&square(0.0, 0.0, 10.0));
        assert_eq!(faces.len(), 1);
        assert!((total_area(&faces) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn polygonize_assigns_the_hole_to_its_shell() {
        let mut lines = square(0.0, 0.0, 10.0);
        lines.extend(square(3.0, 3.0, 4.0));
        let faces = polygonize(&lines);
        assert_eq!(faces.len(), 2);
        assert!((total_area(&faces) - 100.0).abs() < 1e-9);
        assert_eq!(faces.iter().filter(|p| p.interiors().len() == 1).count(), 1);
    }

    #[test]
    fn crossing_squares_partition_the_union() {
        let mut lines = square(0.0, 0.0, 2.0);
        lines.extend(square(1.0, 1.0, 2.0));
        let faces = polygonize(&lines);
        assert_eq!(faces.len(), 3);
        assert!((total_area(&faces) - 7.0).abs() < 1e-9);
    }
}

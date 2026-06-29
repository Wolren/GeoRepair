use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::collections::VecDeque;

#[cfg_attr(not(test), allow(unused_imports))]
use geo::{Coord, Line, LineString};

use crate::core;
use crate::orient::orient2d;

/// ---------------------------------------------------------------------------
/// Graph construction
/// ---------------------------------------------------------------------------
#[inline(always)]
fn snap_key(c: Coord<f64>) -> (i64, i64) {
    let sx = c.x * core::SNAP_SCALE;
    let sy = c.y * core::SNAP_SCALE;
    let x = if sx.is_finite() {
        sx.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0i64
    };
    let y = if sy.is_finite() {
        sy.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0i64
    };
    (x, y)
}

#[inline(always)]
fn key_to_coord(key: (i64, i64)) -> Coord<f64> {
    Coord {
        x: key.0 as f64 / core::SNAP_SCALE,
        y: key.1 as f64 / core::SNAP_SCALE,
    }
}

pub(crate) struct Graph {
    pub(crate) verts: Vec<Coord<f64>>,
    pub(crate) edges: Vec<(usize, usize)>,
    pub(crate) sorted_adj: Vec<SmallVec<[(usize, usize); 4]>>,
}

pub(crate) fn build_graph(lines: &[Line<f64>]) -> Graph {
    let mut key_to_idx: FxHashMap<(i64, i64), usize> = FxHashMap::default();
    let mut verts: Vec<Coord<f64>> = Vec::new();
    let mut get_vert = |c: Coord<f64>| -> usize {
        let key = snap_key(c);
        *key_to_idx.entry(key).or_insert_with(|| {
            let idx = verts.len();
            verts.push(key_to_coord(key));
            idx
        })
    };
    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(lines.len());
    for line in lines {
        let fi = get_vert(line.start);
        let ti = get_vert(line.end);
        if fi != ti {
            edges.push((fi, ti));
        }
    }
    let n_verts = verts.len();
    let mut adj: Vec<SmallVec<[(usize, usize); 4]>> = vec![SmallVec::new(); n_verts];
    for (ei, &(fi, ti)) in edges.iter().enumerate() {
        adj[fi].push((ti, ei));
        adj[ti].push((fi, ei));
    }
    let sorted_adj: Vec<SmallVec<[(usize, usize); 4]>> = adj
        .into_iter()
        .enumerate()
        .map(|(vi, mut neighbors)| {
            let cx = verts[vi].x;
            let cy = verts[vi].y;
            neighbors.sort_by(|(a_idx, _), (b_idx, _)| {
                let aa = (verts[*a_idx].y - cy).atan2(verts[*a_idx].x - cx);
                let ba = (verts[*b_idx].y - cy).atan2(verts[*b_idx].x - cx);
                aa.partial_cmp(&ba).unwrap_or(std::cmp::Ordering::Equal)
            });
            neighbors
        })
        .collect();
    Graph {
        verts,
        edges,
        sorted_adj,
    }
}

/// ---------------------------------------------------------------------------
/// Face extraction — walk each unused directed edge using smallest CCW turn
/// ---------------------------------------------------------------------------
pub(crate) fn extract_all_faces(graph: &Graph) -> Option<Vec<Vec<(usize, usize)>>> {
    let n_edges = graph.edges.len();
    let mut used_fwd = vec![false; n_edges];
    let mut used_rev = vec![false; n_edges];
    let mut faces: Vec<Vec<(usize, usize)>> = Vec::new();

    for start_ei in 0..n_edges {
        let (fi, ti) = graph.edges[start_ei];
        if !used_fwd[start_ei] && let Some(face) = walk_face(graph, start_ei, fi, ti, &mut used_fwd, &mut used_rev)
            && face.len() >= 3 {
                faces.push(face);
            }
        if !used_rev[start_ei] && let Some(face) = walk_face(graph, start_ei, ti, fi, &mut used_fwd, &mut used_rev)
            && face.len() >= 3 {
                faces.push(face);
            }
    }
    if faces.is_empty() {
        None
    } else {
        Some(faces)
    }
}

fn walk_face(
    graph: &Graph,
    start_ei: usize,
    start_from: usize,
    start_to: usize,
    used_fwd: &mut [bool],
    used_rev: &mut [bool],
) -> Option<Vec<(usize, usize)>> {
    let mut face: Vec<(usize, usize)> = Vec::new();
    let mut cur_ei = start_ei;
    let mut cur_to = start_to;
    let mut first = true;

    let start_is_forward = graph.edges[start_ei].0 == start_from;
    let mut used_any_dir = vec![false; graph.edges.len()];

    loop {
        if !first && cur_ei == start_ei && cur_to == start_to {
            break;
        }
        first = false;

        let (from_idx, to_idx) = graph.edges[cur_ei];
        let is_forward = to_idx == cur_to;
        let used = if is_forward {
            &mut *used_fwd
        } else {
            &mut *used_rev
        };
        if used[cur_ei] {
            break;
        }
        used[cur_ei] = true;
        used_any_dir[cur_ei] = true;

        face.push((cur_ei, cur_to));

        let cur_from = if is_forward { from_idx } else { to_idx };
        let incoming_angle = {
            let dx = graph.verts[cur_to].x - graph.verts[cur_from].x;
            let dy = graph.verts[cur_to].y - graph.verts[cur_from].y;
            dy.atan2(dx)
        };

        let next = find_next_edge(
            graph,
            cur_to,
            cur_ei,
            incoming_angle,
            used_fwd,
            used_rev,
            &used_any_dir,
            start_ei,
            start_is_forward,
        );

        match next {
            Some((next_ei, next_to)) => {
                cur_ei = next_ei;
                cur_to = next_to;
            }
            None => break,
        }

        if face.len() > graph.edges.len() * 2 {
            break;
        }
    }

    Some(face)
}

#[allow(clippy::too_many_arguments)]
fn find_next_edge(
    graph: &Graph,
    v_idx: usize,
    incoming_ei: usize,
    incoming_angle: f64,
    used_fwd: &[bool],
    used_rev: &[bool],
    used_any_dir: &[bool],
    start_ei: usize,
    start_is_forward: bool,
) -> Option<(usize, usize)> {
    let mut best: Option<(usize, f64, usize)> = None;

    for &(_n_idx, e_idx) in &graph.sorted_adj[v_idx] {
        if e_idx == incoming_ei {
            continue;
        }

        let (from_idx, to_idx) = graph.edges[e_idx];
        let is_forward = from_idx == v_idx;
        let used = if is_forward {
            used_fwd[e_idx]
        } else {
            used_rev[e_idx]
        };

        if used && e_idx != start_ei {
            continue;
        }
        if used_any_dir[e_idx] && e_idx != start_ei {
            continue;
        }
        if e_idx == start_ei && is_forward != start_is_forward {
            continue;
        }

        let dest = if is_forward { to_idx } else { from_idx };
        let out_angle = (graph.verts[dest].y - graph.verts[v_idx].y)
            .atan2(graph.verts[dest].x - graph.verts[v_idx].x);

        let mut turn = out_angle - incoming_angle;
        if turn < 0.0 {
            if turn > -1e-10 {
                turn = 0.0;
            } else {
                turn += 2.0 * std::f64::consts::PI;
            }
        }

        if best.is_none() || turn < best.unwrap().1 {
            best = Some((e_idx, turn, dest));
        }
    }

    best.map(|(ei, _, to)| (ei, to))
}

/// ---------------------------------------------------------------------------
/// Split face at repeated vertices (pinch points) into simple cycles
/// ---------------------------------------------------------------------------
pub(crate) fn split_face_at_pinch_points(
    face: &[(usize, usize)],
    edges: &[(usize, usize)],
) -> Vec<Vec<(usize, usize)>> {
    split_face_at_pinch_points_depth(face, edges, 64)
}

fn split_face_at_pinch_points_depth(
    face: &[(usize, usize)],
    edges: &[(usize, usize)],
    depth: usize,
) -> Vec<Vec<(usize, usize)>> {
    if depth == 0 {
        return vec![face.to_vec()];
    }
    let to_verts: Vec<usize> = face.iter().map(|&(_, to)| to).collect();
    let n = to_verts.len();

    let (first_from, first_to) = edges[face[0].0];
    let start_vert = if first_to == face[0].1 {
        first_from
    } else {
        first_to
    };

    if n >= 2 && to_verts[n - 1] != start_vert {
        let last_to = to_verts[n - 1];
        for (ei, &(from, to)) in edges.iter().enumerate() {
            if from == last_to && to == start_vert {
                let mut closed = face.to_vec();
                closed.push((ei, to));
                return split_face_at_pinch_points_depth(&closed, edges, depth - 1);
            }
            if to == last_to && from == start_vert {
                let mut closed = face.to_vec();
                closed.push((ei, from));
                return split_face_at_pinch_points_depth(&closed, edges, depth - 1);
            }
        }
    }

    if n < 3 {
        return vec![face.to_vec()];
    }

    let max_id = to_verts
        .iter()
        .copied()
        .max()
        .unwrap_or(start_vert)
        .max(start_vert);
    let mut first_seen = vec![None; max_id + 1];
    first_seen[start_vert] = Some(0);

    for j in 0..n {
        let v = to_verts[j];
        if let Some(i) = first_seen[v] {
            if i == 0 && j == n - 1 {
                continue;
            }
            if i == 0 {
                let sub1: Vec<(usize, usize)> = face[0..=j].to_vec();
                let sub2: Vec<(usize, usize)> = face[j + 1..].to_vec();
                let mut result = split_face_at_pinch_points_depth(&sub1, edges, depth - 1);
                result.extend(split_face_at_pinch_points_depth(&sub2, edges, depth - 1));
                return result;
            }
            let sub1: Vec<(usize, usize)> = face[i + 1..=j].to_vec();
            let sub2: Vec<(usize, usize)> = face[j + 1..]
                .iter()
                .chain(face[0..=i].iter())
                .copied()
                .collect();
            let mut result = split_face_at_pinch_points_depth(&sub1, edges, depth - 1);
            result.extend(split_face_at_pinch_points_depth(&sub2, edges, depth - 1));
            return result;
        }
        first_seen[v] = Some(j + 1);
    }

    vec![face.to_vec()]
}

fn face_winding(
    face: &[(usize, usize)],
    verts: &[Coord<f64>],
    graph_edges: &[(usize, usize)],
    input_ring: &[Coord<f64>],
) -> i32 {
    let &(ei, to_idx) = &face[0];
    let (from_vi, to_vi) = graph_edges[ei];
    let (start_coord, end_coord) = if to_vi == to_idx {
        (verts[from_vi], verts[to_vi])
    } else {
        (verts[to_vi], verts[from_vi])
    };

    let dx = end_coord.x - start_coord.x;
    let dy = end_coord.y - start_coord.y;
    let len = (dx * dx + dy * dy).sqrt().max(1e-16);
    let mx = (start_coord.x + end_coord.x) * 0.5;
    let my = (start_coord.y + end_coord.y) * 0.5;
    let cx = mx + (-dy / len) * 1e-7;
    let cy = my + (dx / len) * 1e-7;

    let mut wn = 0i32;
    for i in 0..input_ring.len() - 1 {
        let a = input_ring[i];
        let b = input_ring[i + 1];
        if a.y <= cy {
            if b.y > cy && orient2d(a, b, Coord { x: cx, y: cy }) > 0.0 {
                wn += 1;
            }
        } else if b.y <= cy && orient2d(a, b, Coord { x: cx, y: cy }) < 0.0 {
            wn -= 1;
        }
    }
    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_FIX_RING").is_ok() {
        let vert_indices: Vec<usize> = face.iter().map(|&(_, to)| to).collect();
        eprintln!(
            "  face_winding: verts={:?}, test=({:.10},{:.10}), wn={}",
            vert_indices, cx, cy, wn
        );
    }
    wn
}

/// ---------------------------------------------------------------------------
/// Face labeling: BFS from exterior face toggling interior/exterior
/// ---------------------------------------------------------------------------
pub(crate) fn label_interior_faces(
    edges: &[Line<f64>],
    verts: &[Coord<f64>],
    input_ring: &[Coord<f64>],
    faces: &[Vec<(usize, usize)>],
    graph_edges: &[(usize, usize)],
) -> Option<FxHashSet<usize>> {
    let n_faces = faces.len();
    if n_faces == 0 {
        return None;
    }

    let mut edge_to_faces: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for (fi, face) in faces.iter().enumerate() {
        for &(ei, _) in face {
            edge_to_faces.entry(ei).or_default().push(fi);
        }
    }
    let mut adj: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for faces_on_edge in edge_to_faces.values() {
        if faces_on_edge.len() == 2 {
            adj.entry(faces_on_edge[0])
                .or_default()
                .push(faces_on_edge[1]);
            adj.entry(faces_on_edge[1])
                .or_default()
                .push(faces_on_edge[0]);
        }
    }

    let exterior = {
        let mut best: Option<(usize, f64)> = None;
        for (fi, face) in faces.iter().enumerate() {
            let (mut min_x, mut max_x, mut min_y, mut max_y) =
                (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for &(ei, _) in face {
                let e = &edges[ei];
                min_x = min_x.min(e.start.x).min(e.end.x);
                max_x = max_x.max(e.start.x).max(e.end.x);
                min_y = min_y.min(e.start.y).min(e.end.y);
                max_y = max_y.max(e.start.y).max(e.end.y);
            }
            let area = (max_x - min_x) * (max_y - min_y);
            if best.is_none_or(|(_, a)| area > a) {
                best = Some((fi, area));
            }
        }
        best.map(|(i, _)| i)?
    };

    let mut interior: FxHashSet<usize> = FxHashSet::default();
    let mut visited: FxHashSet<usize> = FxHashSet::default();
    let mut queue: VecDeque<(usize, bool)> = VecDeque::new();
    visited.insert(exterior);
    queue.push_back((exterior, false));

    while let Some((face, is_interior)) = queue.pop_front() {
        if is_interior {
            interior.insert(face);
        }
        if let Some(neighbors) = adj.get(&face) {
            for &nb in neighbors {
                if visited.insert(nb) {
                    queue.push_back((nb, !is_interior));
                }
            }
        }
    }

    let mut to_remove = Vec::new();
    for &fi in &interior {
        let face = &faces[fi];
        let wn = face_winding(face, verts, graph_edges, input_ring);
        if wn == 0 {
            to_remove.push(fi);
        }
    }
    for fi in to_remove {
        interior.remove(&fi);
    }

    if visited.len() < n_faces || {
        let ext_face = &faces[exterior];
        let wn = face_winding(ext_face, verts, graph_edges, input_ring);
        wn != 0
    } {
        for (fi, face) in faces.iter().enumerate() {
            if interior.contains(&fi) {
                continue;
            }

            let wn = face_winding(face, verts, graph_edges, input_ring);
            if wn != 0 {
                interior.insert(fi);
            }
        }
    }

    Some(interior)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::fix_ring::{
        basic_cleanup, edges_from_coords, has_self_intersections, repair_ring, split_edges,
    };

    fn ring_area(ring: &LineString<f64>) -> f64 {
        let mut s = 0.0;
        for w in ring.0.windows(2) {
            s += w[0].x * w[1].y - w[1].x * w[0].y;
        }
        s.abs() / 2.0
    }

    fn total_area(rings: &[LineString<f64>]) -> f64 {
        rings.iter().map(ring_area).sum()
    }

    #[test]
    fn test_square() {
        let ring = ls(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        let input_area = ring_area(&ring);
        let r = repair_ring(&ring);
        assert!(r.is_some());
        let rings = r.unwrap();
        assert_eq!(rings.len(), 1);
        let output_area = total_area(&rings);
        if input_area > 0.0 {
            assert!(
                (output_area / input_area - 1.0).abs() < 0.5,
                "square area ratio {:.4}",
                output_area / input_area
            );
        }
    }

    #[test]
    fn test_bowtie() {
        let ring = ls(&[
            (0.0, 0.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        let input_area = ring_area(&ring);
        let r = repair_ring(&ring);
        assert!(r.is_some(), "bowtie should produce result");
        let rings = r.unwrap();
        assert!(!rings.is_empty(), "bowtie should produce at least one ring");
        for ring in &rings {
            assert!(ring.0.len() >= 4, "ring too short");
            assert_eq!(ring.0.first(), ring.0.last(), "ring not closed");
        }
        let output_area = total_area(&rings);
        if input_area > 0.0 {
            assert!(
                (output_area / input_area - 1.0).abs() < 0.5,
                "bowtie area ratio {:.4}",
                output_area / input_area
            );
        } else {
            assert!(
                output_area > 0.0,
                "bowtie output should have positive area, got {:.0}",
                output_area
            );
        }
    }

    #[test]
    fn test_empty() {
        let ring = LineString::<f64>::new(Vec::new());
        assert!(repair_ring(&ring).is_none());
    }

    #[test]
    fn test_square_two_faces() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 1.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
            Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 0.0, y: 1.0 }),
            Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 0.0, y: 0.0 }),
        ];
        let graph = build_graph(&edges);
        let faces = extract_all_faces(&graph);
        assert!(faces.is_some());
        assert_eq!(faces.unwrap().len(), 2);
    }

    #[test]
    fn test_has_self_intersections_true() {
        let coords = coords(&[
            (0.0, 0.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        assert!(has_self_intersections(&coords));
    }

    #[test]
    fn test_has_self_intersections_false() {
        let coords = coords(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        assert!(!has_self_intersections(&coords));
    }

    #[test]
    fn test_split_edges_crossing() {
        let edges = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 2.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 0.0, y: 2.0 }),
        ];
        let result = split_edges(&edges);
        assert!(
            result.len() >= 4,
            "crossing edges should split: got {}",
            result.len()
        );
    }

    #[test]
    fn test_three_lobes() {
        let ring = ls(&[
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 0.0),
        ]);
        let r = repair_ring(&ring);
        assert!(r.is_some());
        let rings = r.unwrap();
        assert!(!rings.is_empty());
        for ring in &rings {
            assert!(ring.0.len() >= 4);
            assert_eq!(ring.0.first(), ring.0.last());
        }
    }

    #[test]
    fn test_large_coords() {
        let ring = ls(&[
            (0.0, 0.0),
            (1_000_000.0, 1_000_000.0),
            (1_000_000.0, 0.0),
            (0.0, 1_000_000.0),
            (0.0, 0.0),
        ]);
        let r = repair_ring(&ring);
        assert!(r.is_some());
    }

    #[test]
    fn diagnose_fuzz_failure() {
        let ring = ls(&[
            (-32.94925304356217, -37.4509724868373),
            (25.087850997208253, -29.87382634047737),
            (0.0, -48.64262720158944),
            (-40.61251938421724, -45.1172049629247),
            (-38.51974407936723, -13.433918287897887),
            (-16.8110711840133, -46.226614473001),
        ]);
        let coords = basic_cleanup(&ring).unwrap();
        eprintln!("after cleanup: {} coords", coords.len());
        for (i, c) in coords.iter().enumerate() {
            eprintln!("  coords[{}]: ({}, {})", i, c.x, c.y);
        }
        let si = has_self_intersections(&coords);
        eprintln!("has_self_intersections: {}", si);
        if !si {
            return;
        }

        let edges = edges_from_coords(&coords);
        eprintln!("edges: {}", edges.len());
        let noded = split_edges(&edges);
        eprintln!("noded edges: {}", noded.len());
        for (i, e) in noded.iter().enumerate() {
            eprintln!(
                "  e[{}]: ({},{}) -> ({},{})",
                i, e.start.x, e.start.y, e.end.x, e.end.y
            );
        }
        let graph = build_graph(&noded);
        eprintln!(
            "graph: {} verts, {} edges",
            graph.verts.len(),
            graph.edges.len()
        );
        for (i, v) in graph.verts.iter().enumerate() {
            eprintln!("  v[{}]: ({}, {})", i, v.x, v.y);
        }
        for (i, (fi, ti)) in graph.edges.iter().enumerate() {
            eprintln!("  edge[{}]: {} -> {}", i, fi, ti);
        }

        let faces = extract_all_faces(&graph).unwrap();
        eprintln!("extracted {} faces", faces.len());
        for (fi, face) in faces.iter().enumerate() {
            eprintln!("  face[{}]: {} edges", fi, face.len());
            for &(ei, to) in face {
                eprint!(" (e{},v{})", ei, to);
            }
            eprintln!();
            let mut ring: Vec<Coord<f64>> = face.iter().map(|&(_, to)| graph.verts[to]).collect();
            if ring.len() >= 3 {
                ring.push(ring[0]);
            }
            let check_si = has_self_intersections(&ring);
            eprintln!("    self-intersecting boundary: {}", check_si);
        }

        let simple_faces: Vec<Vec<(usize, usize)>> = faces
            .iter()
            .flat_map(|f| split_face_at_pinch_points(f, &graph.edges))
            .filter(|f| f.len() >= 3)
            .collect();
        eprintln!("after pinch-split: {} simple faces", simple_faces.len());
        for (fi, face) in simple_faces.iter().enumerate() {
            eprintln!("  simple_face[{}]: {} edges", fi, face.len());
            let mut ring: Vec<Coord<f64>> = face.iter().map(|&(_, to)| graph.verts[to]).collect();
            if ring.len() >= 3 {
                ring.push(ring[0]);
            }
            let check_si = has_self_intersections(&ring);
            eprintln!("    self-intersecting boundary: {}", check_si);
        }

        let interior =
            label_interior_faces(&noded, &graph.verts, &coords, &simple_faces, &graph.edges)
                .unwrap();
        eprintln!("interior faces: {:?}", interior);
        for &fi in &interior {
            let face = &simple_faces[fi];
            let mut ring_coords: Vec<Coord<f64>> = face
                .iter()
                .map(|&(_, to_idx)| graph.verts[to_idx])
                .collect();
            eprintln!("  interior face[{}]: {} coords", fi, ring_coords.len());
            if ring_coords.len() >= 3 {
                ring_coords.push(ring_coords[0]);
            }
            let check_si = has_self_intersections(&ring_coords);
            eprintln!("    self-intersecting: {}", check_si);
            for (i, c) in ring_coords.iter().enumerate() {
                eprintln!("    ring[{}]: ({}, {})", i, c.x, c.y);
            }
        }
    }

    fn ls(pairs: &[(f64, f64)]) -> LineString<f64> {
        LineString::new(pairs.iter().map(|&(x, y)| Coord { x, y }).collect())
    }

    fn coords(pairs: &[(f64, f64)]) -> Vec<Coord<f64>> {
        pairs.iter().map(|&(x, y)| Coord { x, y }).collect()
    }
}

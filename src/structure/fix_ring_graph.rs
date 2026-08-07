
use alloc::vec::Vec;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

#[cfg_attr(not(test), allow(unused_imports))]
use geo::{Coord, Line, LineString};

use crate::core;

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

pub struct Graph {
    pub verts: Vec<Coord<f64>>,
    pub edges: Vec<(usize, usize)>,
    pub sorted_adj: Vec<SmallVec<[(usize, usize); 4]>>,
}

pub fn build_graph(lines: &[Line<f64>]) -> Graph {
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
                aa.partial_cmp(&ba).unwrap_or(::core::cmp::Ordering::Equal)
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
/// Face extraction - faithful port of GEOS PolygonizeGraph::getEdgeRings
/// (computeNextCWEdges + findLabeledEdgeRings + convertMaximalToMinimalEdgeRings,
/// then findEdgeRing). Labels are per-DIRECTED edge (2*n_edges entries): the two
/// faces on either side of an undirected edge carry different labels, and the
/// CCW conversion distinguishes de->getLabel() from sym->getLabel().
/// Without this, faces merge across shared edges (measured: 7 faces instead
/// of the correct 10 on a 9-vertex multi-crossing ring).
/// ---------------------------------------------------------------------------
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn extract_all_faces_geos(graph: &Graph) -> Option<Vec<Vec<(usize, usize)>>> {
    let n_edges = graph.edges.len();
    // Directed edge indexing: idx e = forward (from->to), idx n_edges+e = reverse.
    // next[i] follows GEOS's PolygonizeDirectedEdge::next.
    let mut next: Vec<usize> = vec![0; n_edges * 2];

    // ---- Step 1: computeNextCWEdges (maximal rings) -----------------------
    // Edges around each node are stored in CCW order in sorted_adj (atan2 sort).
    // For consecutive edges prev, curr in CCW order: sym(prev).next = curr.
    // This yields the CW-facing rings (the "maximal" rings of the graph).
    for (v, neighbors) in graph.sorted_adj.iter().enumerate() {
        let deg = neighbors.len();
        if deg == 0 {
            continue;
        }
        if deg == 1 {
            // Dangle: reverse of the single edge points back to its forward.
            let (_, e) = neighbors[0];
            let (fwd, rev) = dir_at_v(graph, e, v);
            next[rev] = fwd;
            continue;
        }
        // neighbors sorted CCW by atan2. For i, prev = (i+deg-1)%deg.
        for i in 0..deg {
            let (_, e_curr) = neighbors[i];
            let (_, e_prev) = neighbors[(i + deg - 1) % deg];
            let (_, prev_rev) = dir_at_v(graph, e_prev, v);
            let (curr_fwd, _) = dir_at_v(graph, e_curr, v);
            next[prev_rev] = curr_fwd;
        }
    }

    // ---- Step 2: findLabeledEdgeRings --------------------------------------
    // Walk CW rings via `next`; assign one label per ring. Labels are stored
    // PER DIRECTED EDGE (the sym of an edge belongs to the adjacent ring).
    let mut label: Vec<i64> = vec![-1; n_edges * 2];
    let mut next_label: i64 = 1;
    for start in 0..n_edges * 2 {
        if label[start] >= 0 {
            continue;
        }
        // Walk the maximal ring containing `start`.
        let mut cur = start;
        let mut ring: Vec<usize> = Vec::new();
        loop {
            if label[cur] >= 0 {
                break;
            }
            label[cur] = next_label;
            ring.push(cur);
            let nxt = next[cur];
            if nxt == cur || nxt == start {
                break;
            }
            cur = nxt;
            if ring.len() > n_edges * 2 {
                break;
            }
        }
        next_label += 1;
    }

    // ---- Step 3: convertMaximalToMinimalEdgeRings --------------------------
    // For each labeled ring, rewire `next` at its intersection nodes
    // (nodes with degree > 1 within that label) via computeNextCCWEdges.
    // This splits maximal rings into minimal rings (true faces).
    let mut ccw_next: Vec<usize> = next.clone();
    for v in 0..graph.verts.len() {
        let neighbors = &graph.sorted_adj[v];
        let deg = neighbors.len();
        if deg < 2 {
            continue;
        }
        // Collect distinct labels present at this node.
        let mut labels_at_node: Vec<i64> = Vec::new();
        for &(_, e) in neighbors {
            let (de, sym) = dir_at_v(graph, e, v);
            for &d in &[de, sym] {
                let lbl = label[d];
                if lbl >= 0 && !labels_at_node.contains(&lbl) {
                    labels_at_node.push(lbl);
                }
            }
        }
        for lbl in labels_at_node {
            // computeNextCCWEdges(node, label): iterate edges in CW order
            // (reverse of CCW sorted_adj). For each edge: if de has the label
            // it's an outgoing edge; if sym has the label it's incoming.
            let mut first_out: Option<usize> = None;
            let mut prev_in: Option<usize> = None;
            for i in (0..deg).rev() {
                let (_, e) = neighbors[i];
                // CRITICAL: the edge may be stored (v->to) or (to->v). Use
                // dir_at_v to resolve which index is the forward (outgoing)
                // direction at v and which is the sym (incoming).
                let (de, sym) = dir_at_v(graph, e, v);
                let mut out_de: Option<usize> = None;
                if label[de] == lbl {
                    out_de = Some(de);
                }
                let mut in_de: Option<usize> = None;
                if label[sym] == lbl {
                    in_de = Some(sym);
                }
                if out_de.is_none() && in_de.is_none() {
                    continue;
                }
                if let Some(ind) = in_de {
                    prev_in = Some(ind);
                }
                if let Some(outd) = out_de {
                    if let Some(pi) = prev_in.take() {
                        ccw_next[pi] = outd;
                    }
                    if first_out.is_none() {
                        first_out = Some(outd);
                    }
                }
            }
            if let Some(pi) = prev_in
                && let Some(fo) = first_out
            {
                ccw_next[pi] = fo;
            }
        }
    }

    // ---- Step 4: findEdgeRing — walk minimal CCW rings ---------------------
    let mut used = vec![false; n_edges * 2];
    let mut faces: Vec<Vec<(usize, usize)>> = Vec::new();
    for start in 0..n_edges * 2 {
        if used[start] {
            continue;
        }
        let mut cur = start;
        let mut face: Vec<(usize, usize)> = Vec::new();
        loop {
            if used[cur] {
                break;
            }
            used[cur] = true;
            let ei = cur % n_edges;
            let (from, to) = graph.edges[ei];
            let to_vert = if cur < n_edges { to } else { from };
            face.push((ei, to_vert));
            let nxt = ccw_next[cur];
            if nxt == cur || nxt == start {
                break;
            }
            cur = nxt;
            if face.len() > n_edges * 2 {
                break;
            }
        }
        if face.len() >= 3 {
            faces.push(face);
        }
    }

    if faces.is_empty() {
        return None;
    }
    Some(faces)
}

#[inline(always)]
fn dir_at_v(graph: &Graph, e: usize, v: usize) -> (usize, usize) {
    let n_edges = graph.edges.len();
    let (from, _to) = graph.edges[e];
    if from == v {
        (e, n_edges + e) // fwd = e, rev = n_edges+e
    } else {
        (n_edges + e, e)
    }
}

pub fn extract_all_faces(graph: &Graph) -> Option<Vec<Vec<(usize, usize)>>> {
    let n_edges = graph.edges.len();
    let mut used_fwd = vec![false; n_edges];
    let mut used_rev = vec![false; n_edges];
    let mut faces: Vec<Vec<(usize, usize)>> = Vec::new();

    for start_ei in 0..n_edges {
        let (fi, ti) = graph.edges[start_ei];
        if !used_fwd[start_ei]
            && let Some(face) = walk_face(graph, start_ei, fi, ti, &mut used_fwd, &mut used_rev)
            && face.len() >= 3
        {
            faces.push(face);
        }
        if !used_rev[start_ei]
            && let Some(face) = walk_face(graph, start_ei, ti, fi, &mut used_fwd, &mut used_rev)
            && face.len() >= 3
        {
            faces.push(face);
        }
    }
    if faces.is_empty() { None } else { Some(faces) }
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
                turn += 2.0 * ::core::f64::consts::PI;
            }
        }

        if best.is_none_or(|(_, t, _)| turn < t) {
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
}/// ---------------------------------------------------------------------------
/// Face labeling: even-odd winding parity (GEOS MakeValidPoly equivalent)
/// ---------------------------------------------------------------------------
/// GEOS's BuildArea + symdiff loop selects the faces whose winding number
/// w.r.t. the input ring is ODD. The BFS-from-exterior toggle is WRONG for
/// multi-crossing rings: with 2+ crossings, faces on the far side of a
/// second crossing are reachable through an odd number of edges but have
/// EVEN winding (measured: 5 of 9 faces kept, GEOS area 9931.89 vs BFS
/// interior 11956). Winding parity is exact:
///   face winding = winding number of any interior probe point vs the ring.
pub fn label_interior_faces(
    _edges: &[Line<f64>],
    verts: &[Coord<f64>],
    input_ring: &[Coord<f64>],
    faces: &[Vec<(usize, usize)>],
    graph_edges: &[(usize, usize)],
) -> Option<FxHashSet<usize>> {
    let mut interior: FxHashSet<usize> = FxHashSet::default();
    for (fi, face) in faces.iter().enumerate() {
        let probe = face_probe_point(face, verts, graph_edges)?;
        let wn = winding_number(input_ring, probe);
        if wn % 2 != 0 {
            interior.insert(fi);
        }
    }
    Some(interior)
}

/// Interior probe point for a face: midpoint of the first edge, nudged
/// perpendicular toward the face interior. For a face of positive area this
/// point is strictly inside (a centroid can land outside concave faces).
fn face_probe_point(
    face: &[(usize, usize)],
    verts: &[Coord<f64>],
    graph_edges: &[(usize, usize)],
) -> Option<Coord<f64>> {
    if face.len() < 3 {
        return None;
    }
    // First edge of the face: (v0 -> v1) where v1 = to0 is the traversal
    // destination. The source vertex is the OTHER endpoint of the edge.
    let (e0, to0) = face[0];
    let (from0, to1) = graph_edges[e0];
    let v0 = if to1 == to0 { verts[from0] } else { verts[to1] };
    let v1 = verts[to0];
    let mid = Coord {
        x: (v0.x + v1.x) * 0.5,
        y: (v0.y + v1.y) * 0.5,
    };
    // WALKER CONVENTION: every face (bounded AND outer) is walked with the
    // face interior on the RIGHT of the directed edge. Bounded faces come
    // out CW (negative shoelace), the outer face comes out CCW (positive)
    // — but both have their interior on the right. So the nudge is ALWAYS
    // to the right; the shoelace sign alone would push the outer face probe
    // INTO the ring (winding 1 → mislabeled interior, measured).
    let scale = (mid.x.abs().max(mid.y.abs())).max(1.0);
    let eps = 1e-9 * scale;
    let dx = v1.x - v0.x;
    let dy = v1.y - v0.y;
    let len = (dx * dx + dy * dy).sqrt().max(1e-12);
    let (nx, ny) = (-dy / len, dx / len); // left normal
    let sign = -1.0; // interior is on the RIGHT of the directed edge
    Some(Coord {
        x: mid.x + sign * nx * eps,
        y: mid.y + sign * ny * eps,
    })
}

/// Winding number of a point w.r.t. a closed ring (even-odd rule).
fn winding_number(ring: &[Coord<f64>], pt: Coord<f64>) -> i32 {
    let mut wn = 0i32;
    for w in ring.windows(2) {
        let (p1, p2) = (w[0], w[1]);
        if p1.y <= pt.y {
            if p2.y > pt.y {
                let cross = (p2.x - p1.x) * (pt.y - p1.y) - (p2.y - p1.y) * (pt.x - p1.x);
                if cross > 0.0 {
                    wn += 1;
                }
            }
        } else if p2.y <= pt.y {
            let cross = (p2.x - p1.x) * (pt.y - p1.y) - (p2.y - p1.y) * (pt.x - p1.x);
            if cross < 0.0 {
                wn -= 1;
            }
        }
    }
    wn
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "fix_ring_graph_tests.rs"]
mod tests;


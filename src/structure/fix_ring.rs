use std::collections::{HashMap, HashSet, VecDeque};

use geo::Buffer;
use geo::{Coord, Line, LineString, MultiPolygon, Polygon};

pub(crate) fn repair_ring(ring: &LineString<f64>) -> Option<Vec<LineString<f64>>> {
    let coords = basic_cleanup(ring)?;
    if coords.len() < 4 {
        return None;
    }
    if !has_self_intersections(&coords) {
        return Some(vec![LineString::new(coords)]);
    }

    let n = coords.len();

    // For very large self-intersecting rings, skip planar graph (O(n²) face splitting)
    // and let the caller fall back to CDT arrange.
    const LARGE_RING_THRESHOLD: usize = 25000;
    if n > LARGE_RING_THRESHOLD {
        return None;
    }

    // Try buffer(0) first — GEOS-style robust single-pass ring fixer
    let clean_ring = LineString::new(coords.clone());
    if let Some(rings) = buffer_by_zero_repair(&clean_ring) {
        return Some(rings);
    }
    // Fall back to planar graph face extraction
    fix_self_intersecting(&coords)
}

/// GEOS `bufferByZero()` equivalent: wraps the ring as a Polygon and buffers by 0.
/// The buffer operation naturally resolves self-intersections and produces valid OGC output.
fn buffer_by_zero_repair(ring: &LineString<f64>) -> Option<Vec<LineString<f64>>> {
    let poly = Polygon::new(ring.clone(), Vec::new());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poly.buffer(0.0)));
    match result {
        Ok(mp) if !mp.0.is_empty() => {
            let rings: Vec<LineString<f64>> =
                mp.0.into_iter()
                    .flat_map(|p| {
                        let mut rings = vec![p.exterior().clone()];
                        rings.extend(p.interiors().iter().cloned());
                        rings
                    })
                    .collect();
            if rings.is_empty() {
                None
            } else {
                Some(rings)
            }
        }
        _ => None,
    }
}

fn basic_cleanup(ring: &LineString<f64>) -> Option<Vec<Coord<f64>>> {
    let coords: Vec<_> = ring
        .0
        .iter()
        .copied()
        .filter(|c| c.x.is_finite() && c.y.is_finite())
        .collect();
    if coords.is_empty() {
        return None;
    }
    let mut deduped = remove_consecutive_duplicates(coords);
    if deduped.is_empty() {
        return None;
    }
    if deduped.first() != deduped.last() {
        deduped.push(deduped[0]);
    }
    if deduped.len() < 4 {
        return None;
    }
    Some(deduped)
}

fn remove_consecutive_duplicates(coords: Vec<Coord<f64>>) -> Vec<Coord<f64>> {
    let mut result = Vec::with_capacity(coords.len());
    for c in coords {
        if result.last() != Some(&c) {
            result.push(c);
        }
    }
    result
}

fn has_self_intersections(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 4 {
        return false;
    }

    // Check for duplicate non-consecutive vertices (pinch points) via HashSet
    let mut seen: HashSet<(u64, u64)> = HashSet::with_capacity(n);
    for i in 0..n - 1 {
        let key = (coords[i].x.to_bits(), coords[i].y.to_bits());
        if !seen.insert(key) {
            return true;
        }
    }

    // Spatial grid to accelerate edge intersection checks
    if n > 500 {
        return has_self_intersections_grid(coords);
    }

    has_self_intersections_bruteforce(coords)
}

/// O(n²) brute force for small rings
fn has_self_intersections_bruteforce(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    for i in 0..n - 1 {
        for j in i + 2..n - 1 {
            if i == 0 && j == n - 2 {
                continue;
            }
            if check_edge_pair(coords, i, j) {
                return true;
            }
        }
    }
    false
}

/// O(n log n) expected via uniform spatial grid — for large rings
fn has_self_intersections_grid(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    let n_edges = n - 1;

    // Compute bounding box
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for c in coords {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }

    let eps = 1e-12;
    let dx = max_x - min_x;
    let dy = max_y - min_y;

    // Degenerate bbox: fall back to brute force
    if dx < eps || dy < eps {
        return has_self_intersections_bruteforce(coords);
    }

    // Grid size: sqrt(n) × sqrt(n), at least 4, at most 256
    let grid_dim = (n_edges as f64).sqrt().ceil() as usize;
    let grid_dim = grid_dim.max(4).min(256);
    let cell_w = dx / grid_dim as f64;
    let cell_h = dy / grid_dim as f64;

    // Grid: each cell stores list of edge indices
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); grid_dim * grid_dim];

    for ei in 0..n_edges {
        let a = coords[ei];
        let b = coords[ei + 1];
        let lo = (a.x.min(b.x) - min_x) / cell_w;
        let hi = (a.x.max(b.x) - min_x) / cell_w;
        let min_cx = (lo.floor() as isize).max(0) as usize;
        let max_cx = (hi.ceil() as isize - 1).min(grid_dim as isize - 1).max(0) as usize;
        let lo = (a.y.min(b.y) - min_y) / cell_h;
        let hi = (a.y.max(b.y) - min_y) / cell_h;
        let min_cy = (lo.floor() as isize).max(0) as usize;
        let max_cy = (hi.ceil() as isize - 1).min(grid_dim as isize - 1).max(0) as usize;

        for cx in min_cx..=max_cx {
            for cy in min_cy..=max_cy {
                grid[cx + cy * grid_dim].push(ei);
            }
        }
    }

    // For each pair of edges in the same cell, check for intersection
    // Use a Set to avoid checking the same pair twice
    let mut checked: HashSet<(usize, usize)> = HashSet::new();

    for cell in &grid {
        if cell.len() < 2 {
            continue;
        }
        // Sort to enable early exit when j is far from i
        let mut sorted = cell.clone();
        sorted.sort_unstable();
        for ii in 0..sorted.len() {
            let ei = sorted[ii];
            for jj in (ii + 1)..sorted.len() {
                let ej = sorted[jj];
                if !checked.insert((ei, ej)) {
                    continue;
                }
                // Skip consecutive edges and the first/last edge pair
                if ei.abs_diff(ej) <= 1 || (ei == 0 && ej == n_edges - 1) {
                    continue;
                }
                if check_edge_pair(coords, ei, ej) {
                    return true;
                }
            }
        }
    }

    false
}

/// Check a single pair of edges for any type of intersection
fn check_edge_pair(coords: &[Coord<f64>], i: usize, j: usize) -> bool {
    if edges_cross_proper(coords[i], coords[i + 1], coords[j], coords[j + 1]) {
        return true;
    }
    if vertex_on_edge(coords[i], coords[j], coords[j + 1]) {
        return true;
    }
    if vertex_on_edge(coords[i + 1], coords[j], coords[j + 1]) {
        return true;
    }
    if vertex_on_edge(coords[j], coords[i], coords[i + 1]) {
        return true;
    }
    if vertex_on_edge(coords[j + 1], coords[i], coords[i + 1]) {
        return true;
    }
    if edges_collinear_overlap(coords[i], coords[i + 1], coords[j], coords[j + 1]) {
        return true;
    }
    false
}

fn vertex_on_edge(v: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> bool {
    let eps = 1e-12;
    if orient2d(a, b, v).abs() > eps {
        return false;
    }
    if v == a || v == b {
        return false;
    }
    let between_x = (a.x - b.x).abs() > eps && v.x > a.x.min(b.x) + eps && v.x < a.x.max(b.x) - eps;
    let between_y = (a.y - b.y).abs() > eps && v.y > a.y.min(b.y) + eps && v.y < a.y.max(b.y) - eps;
    between_x || between_y
}

fn edges_collinear_overlap(a1: Coord<f64>, a2: Coord<f64>, b1: Coord<f64>, b2: Coord<f64>) -> bool {
    let eps = 1e-12;
    let o1 = orient2d(a1, a2, b1);
    let o2 = orient2d(a1, a2, b2);
    let o3 = orient2d(b1, b2, a1);
    let o4 = orient2d(b1, b2, a2);
    if o1.abs() > eps || o2.abs() > eps || o3.abs() > eps || o4.abs() > eps {
        return false;
    }
    let lo_x = a1.x.min(a2.x).max(b1.x.min(b2.x));
    let hi_x = a1.x.max(a2.x).min(b1.x.max(b2.x));
    let lo_y = a1.y.min(a2.y).max(b1.y.min(b2.y));
    let hi_y = a1.y.max(a2.y).min(b1.y.max(b2.y));
    lo_x + eps < hi_x && lo_y + eps < hi_y
}

fn orient2d(a: Coord<f64>, b: Coord<f64>, c: Coord<f64>) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn edges_cross_proper(a1: Coord<f64>, a2: Coord<f64>, b1: Coord<f64>, b2: Coord<f64>) -> bool {
    let eps = 1e-12;
    let o1 = orient2d(a1, a2, b1);
    let o2 = orient2d(a1, a2, b2);
    let o3 = orient2d(b1, b2, a1);
    let o4 = orient2d(b1, b2, a2);
    if o1.abs() > eps || o2.abs() > eps || o3.abs() > eps || o4.abs() > eps {
        return o1.signum() != o2.signum() && o3.signum() != o4.signum();
    }
    false
}

/// ---------------------------------------------------------------------------
/// Self-intersecting ring fixer
/// ---------------------------------------------------------------------------

fn fix_self_intersecting(coords: &[Coord<f64>]) -> Option<Vec<LineString<f64>>> {
    let edges = edges_from_coords(coords);
    let noded = split_edges(&edges);
    if noded.is_empty() {
        return None;
    }
    let (graph, verts) = build_graph(&noded);
    if graph.edges.is_empty() {
        return None;
    }
    let faces = extract_all_faces(&graph)?;
    if faces.is_empty() {
        return None;
    }
    // Split faces at pinch points (repeated vertices)
    let simple_faces: Vec<Vec<(usize, usize)>> = faces
        .iter()
        .flat_map(|f| split_face_at_pinch_points(f))
        .filter(|f| f.len() >= 3)
        .collect();
    if simple_faces.is_empty() {
        return None;
    }
    let interior = label_interior_faces(&noded, &simple_faces)?;
    let mut result: Vec<LineString<f64>> = Vec::new();
    for &fi in &interior {
        let face = &simple_faces[fi];
        let mut ring_coords: Vec<Coord<f64>> =
            face.iter().map(|&(_, to_idx)| verts[to_idx]).collect();
        if ring_coords.len() >= 3 {
            ring_coords.push(ring_coords[0]);
            result.push(LineString::new(ring_coords));
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn edges_from_coords(coords: &[Coord<f64>]) -> Vec<Line<f64>> {
    coords.windows(2).map(|w| Line::new(w[0], w[1])).collect()
}

/// ---------------------------------------------------------------------------
/// Edge splitting at intersection points
/// ---------------------------------------------------------------------------

fn split_edges(edges: &[Line<f64>]) -> Vec<Line<f64>> {
    let n = edges.len();
    let mut split_points: Vec<Vec<f64>> = vec![Vec::new(); n];
    let eps = 1e-12;

    if n > 500 {
        split_edges_grid(edges, &mut split_points, eps);
    } else {
        split_edges_bruteforce(edges, &mut split_points, eps);
    }

    let eps_param = 1e-14;
    let mut result = Vec::new();
    for i in 0..n {
        let e = edges[i];
        let mut params: Vec<f64> = std::mem::take(&mut split_points[i]);
        params.sort_by(|a, b| a.partial_cmp(b).unwrap());
        params.dedup_by(|a, b| (*a - *b).abs() < eps_param);
        let mut prev_pt = e.start;
        for &t in &params {
            let pt = lerp(e, t);
            if dist2(pt, prev_pt) > eps_param {
                result.push(Line::new(prev_pt, pt));
            }
            prev_pt = pt;
        }
        if dist2(e.end, prev_pt) > eps_param {
            result.push(Line::new(prev_pt, e.end));
        }
    }
    result
}

fn split_edges_bruteforce(edges: &[Line<f64>], split_points: &mut [Vec<f64>], eps: f64) {
    let n = edges.len();
    for i in 0..n {
        for j in (i + 2)..n {
            if i + 1 == j && edges[i].end == edges[j].start {
                continue;
            }
            if i == 0 && j == n - 1 && edges[i].start == edges[j].end {
                continue;
            }
            if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps) {
                if ti > eps && ti < 1.0 - eps {
                    split_points[i].push(ti);
                }
                if tj > eps && tj < 1.0 - eps {
                    split_points[j].push(tj);
                }
            }
        }
    }
}

fn split_edges_grid(edges: &[Line<f64>], split_points: &mut [Vec<f64>], eps: f64) {
    let n = edges.len();
    let grid = build_edge_grid(edges);
    let mut checked: HashSet<(usize, usize)> = HashSet::new();

    for cell in &grid {
        if cell.len() < 2 {
            continue;
        }
        let mut sorted = cell.clone();
        sorted.sort_unstable();
        for ii in 0..sorted.len() {
            let ei = sorted[ii];
            for jj in (ii + 1)..sorted.len() {
                let ej = sorted[jj];
                if !checked.insert((ei, ej)) {
                    continue;
                }
                if ei.abs_diff(ej) <= 1 || (ei == 0 && ej == n - 1) {
                    continue;
                }
                if let Some((ti, tj)) = intersect_param(&edges[ei], &edges[ej], eps) {
                    if ti > eps && ti < 1.0 - eps {
                        split_points[ei].push(ti);
                    }
                    if tj > eps && tj < 1.0 - eps {
                        split_points[ej].push(tj);
                    }
                }
            }
        }
    }
}

fn build_edge_grid(edges: &[Line<f64>]) -> Vec<Vec<usize>> {
    let n = edges.len();
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for e in edges {
        min_x = min_x.min(e.start.x).min(e.end.x);
        max_x = max_x.max(e.start.x).max(e.end.x);
        min_y = min_y.min(e.start.y).min(e.end.y);
        max_y = max_y.max(e.start.y).max(e.end.y);
    }

    let eps = 1e-12;
    let dx = max_x - min_x;
    let dy = max_y - min_y;
    if dx < eps || dy < eps {
        return vec![(0..n).collect()];
    }

    let grid_dim = (n as f64).sqrt().ceil() as usize;
    let grid_dim = grid_dim.max(4).min(256);
    let cell_w = dx / grid_dim as f64;
    let cell_h = dy / grid_dim as f64;
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); grid_dim * grid_dim];

    for ei in 0..n {
        let e = &edges[ei];
        let lo_x = (e.start.x.min(e.end.x) - min_x) / cell_w;
        let hi_x = (e.start.x.max(e.end.x) - min_x) / cell_w;
        let min_cx = (lo_x.floor() as isize).max(0) as usize;
        let max_cx = (hi_x.ceil() as isize - 1).min(grid_dim as isize - 1).max(0) as usize;
        let lo_y = (e.start.y.min(e.end.y) - min_y) / cell_h;
        let hi_y = (e.start.y.max(e.end.y) - min_y) / cell_h;
        let min_cy = (lo_y.floor() as isize).max(0) as usize;
        let max_cy = (hi_y.ceil() as isize - 1).min(grid_dim as isize - 1).max(0) as usize;

        for cx in min_cx..=max_cx {
            for cy in min_cy..=max_cy {
                grid[cx + cy * grid_dim].push(ei);
            }
        }
    }
    grid
}

fn intersect_param(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
    let denom = (e1.end.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e1.end.y - e1.start.y) * (e2.end.x - e2.start.x);
    if denom.abs() < eps {
        // Near-parallel: check for collinear overlap
        return intersect_param_collinear(e1, e2, eps);
    }
    let t = ((e2.start.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e2.start.y - e1.start.y) * (e2.end.x - e2.start.x))
        / denom;
    let u = ((e2.start.x - e1.start.x) * (e1.end.y - e1.start.y)
        - (e2.start.y - e1.start.y) * (e1.end.x - e1.start.x))
        / denom;
    if t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps {
        Some((t, u))
    } else {
        None
    }
}

/// Handle collinear / near-parallel edge pairs: check if the edges overlap in projection
/// along their shared direction. If they overlap, return the overlap endpoints as split
/// parameters on both edges.
fn intersect_param_collinear(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
    let o1 = orient2d(e1.start, e1.end, e2.start);
    let o2 = orient2d(e1.start, e1.end, e2.end);
    let o3 = orient2d(e2.start, e2.end, e1.start);
    let o4 = orient2d(e2.start, e2.end, e1.end);
    if o1.abs() > eps || o2.abs() > eps || o3.abs() > eps || o4.abs() > eps {
        return None;
    }

    // Project both edges onto e1's direction
    let dx = e1.end.x - e1.start.x;
    let dy = e1.end.y - e1.start.y;
    let len2 = dx * dx + dy * dy;
    if len2 < eps {
        return None;
    }

    let dot = |c: Coord<f64>| -> f64 { (c.x - e1.start.x) * dx + (c.y - e1.start.y) * dy };

    let s1 = (dot(e1.start) / len2).clamp(0.0, 1.0);
    let s2 = (dot(e1.end) / len2).clamp(0.0, 1.0);
    let p1 = (dot(e2.start) / len2).clamp(0.0, 1.0);
    let p2 = (dot(e2.end) / len2).clamp(0.0, 1.0);

    let e1a = s1.min(s2);
    let e1b = s1.max(s2);
    let e2a = p1.min(p2);
    let e2b = p1.max(p2);

    let lo = e1a.max(e2a);
    let hi = e1b.min(e2b);

    if lo + eps < hi {
        // The overlap region on e1 is (lo, hi). Return split parameters on each edge.
        // e1 param: t = lo (first split point)
        // e2 param: project lo back onto e2
        let e2_dot = |c: Coord<f64>| -> f64 {
            let dx2 = e2.end.x - e2.start.x;
            let dy2 = e2.end.y - e2.start.y;
            let len2_2 = dx2 * dx2 + dy2 * dy2;
            if len2_2 < eps {
                return 0.0;
            }
            ((c.x - e2.start.x) * dx2 + (c.y - e2.start.y) * dy2) / len2_2
        };

        // Split point on e1 at parameter lo
        let mid_x = e1.start.x + lo * dx;
        let mid_y = e1.start.y + lo * dy;
        let mid = Coord { x: mid_x, y: mid_y };

        let t_param = lo;
        let u_param = e2_dot(mid).clamp(0.0, 1.0);

        // Only return if the intersection point is not at an endpoint on either edge
        // (parametric tolerances: near 0 or near 1 means endpoint touch, not proper split)
        let e1_eps = eps / dx.abs().max(dy.abs()).max(1.0);
        let e2_eps = eps
            / (e2.end.x - e2.start.x)
                .abs()
                .max((e2.end.y - e2.start.y).abs())
                .max(1.0);

        let on_e1 = t_param > e1_eps && t_param < 1.0 - e1_eps;
        let on_e2 = u_param > e2_eps && u_param < 1.0 - e2_eps;
        if on_e1 || on_e2 {
            return Some((t_param, u_param));
        }
    }
    None
}

fn lerp(e: Line<f64>, t: f64) -> Coord<f64> {
    Coord {
        x: e.start.x + t * (e.end.x - e.start.x),
        y: e.start.y + t * (e.end.y - e.start.y),
    }
}

fn dist2(a: Coord<f64>, b: Coord<f64>) -> f64 {
    (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
}

/// ---------------------------------------------------------------------------
/// Graph construction
/// ---------------------------------------------------------------------------

const SNAP_SCALE: f64 = 1e8;

fn snap_key(c: Coord<f64>) -> (i64, i64) {
    (
        (c.x * SNAP_SCALE).round() as i64,
        (c.y * SNAP_SCALE).round() as i64,
    )
}

fn key_to_coord(key: (i64, i64)) -> Coord<f64> {
    Coord {
        x: key.0 as f64 / SNAP_SCALE,
        y: key.1 as f64 / SNAP_SCALE,
    }
}

struct Graph {
    verts: Vec<Coord<f64>>,
    edges: Vec<(usize, usize)>,
    sorted_adj: Vec<Vec<(usize, usize)>>,
}

fn build_graph(lines: &[Line<f64>]) -> (Graph, Vec<Coord<f64>>) {
    let mut key_to_idx: HashMap<(i64, i64), usize> = HashMap::new();
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
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n_verts];
    for (ei, &(fi, ti)) in edges.iter().enumerate() {
        adj[fi].push((ti, ei));
        adj[ti].push((fi, ei));
    }
    let sorted_adj: Vec<Vec<(usize, usize)>> = adj
        .iter()
        .enumerate()
        .map(|(vi, neighbors)| {
            let cx = verts[vi].x;
            let cy = verts[vi].y;
            let mut sorted = neighbors.clone();
            sorted.sort_by(|(a_idx, _), (b_idx, _)| {
                let aa = (verts[*a_idx].y - cy).atan2(verts[*a_idx].x - cx);
                let ba = (verts[*b_idx].y - cy).atan2(verts[*b_idx].x - cx);
                aa.partial_cmp(&ba).unwrap_or(std::cmp::Ordering::Equal)
            });
            sorted
        })
        .collect();
    (
        Graph {
            verts: verts.clone(),
            edges,
            sorted_adj,
        },
        verts,
    )
}

/// ---------------------------------------------------------------------------
/// Face extraction — walk each unused directed edge using smallest CCW turn
/// ---------------------------------------------------------------------------

fn extract_all_faces(graph: &Graph) -> Option<Vec<Vec<(usize, usize)>>> {
    let n_edges = graph.edges.len();
    let mut used_fwd = vec![false; n_edges];
    let mut used_rev = vec![false; n_edges];
    let mut faces: Vec<Vec<(usize, usize)>> = Vec::new();

    for start_ei in 0..n_edges {
        let (fi, ti) = graph.edges[start_ei];
        if !used_fwd[start_ei] {
            if let Some(face) = walk_face(graph, start_ei, fi, ti, &mut used_fwd, &mut used_rev) {
                if face.len() >= 3 {
                    faces.push(face);
                }
            }
        }
        if !used_rev[start_ei] {
            if let Some(face) = walk_face(graph, start_ei, ti, fi, &mut used_fwd, &mut used_rev) {
                if face.len() >= 3 {
                    faces.push(face);
                }
            }
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
    _start_from: usize,
    start_to: usize,
    used_fwd: &mut [bool],
    used_rev: &mut [bool],
) -> Option<Vec<(usize, usize)>> {
    let mut face: Vec<(usize, usize)> = Vec::new();
    let mut cur_ei = start_ei;
    let mut cur_to = start_to;
    let mut first = true;

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

        face.push((cur_ei, cur_to));

        let cur_from = if is_forward { from_idx } else { to_idx };
        let incoming_angle = {
            let dx = graph.verts[cur_to].x - graph.verts[cur_from].x;
            let dy = graph.verts[cur_to].y - graph.verts[cur_from].y;
            dy.atan2(dx)
        };

        // Find next edge with smallest CCW turn from incoming direction.
        // Allow the start edge as a candidate (it may be used).
        let next = find_next_edge(
            graph,
            cur_to,
            cur_ei,
            incoming_angle,
            used_fwd,
            used_rev,
            start_ei,
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

fn find_next_edge(
    graph: &Graph,
    v_idx: usize,
    incoming_ei: usize,
    incoming_angle: f64,
    used_fwd: &[bool],
    used_rev: &[bool],
    start_ei: usize,
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

        // Skip used edges UNLESS this is the start edge (allows cycle closure)
        if used && e_idx != start_ei {
            continue;
        }

        let dest = if is_forward { to_idx } else { from_idx };
        let out_angle = (graph.verts[dest].y - graph.verts[v_idx].y)
            .atan2(graph.verts[dest].x - graph.verts[v_idx].x);

        let mut turn = out_angle - incoming_angle;
        if turn < 0.0 {
            turn += 2.0 * std::f64::consts::PI;
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

fn split_face_at_pinch_points(face: &[(usize, usize)]) -> Vec<Vec<(usize, usize)>> {
    let verts: Vec<usize> = face.iter().map(|&(_, to)| to).collect();
    let n = verts.len();
    for i in 0..n {
        for j in i + 1..n {
            if verts[i] == verts[j] {
                // Skip normal closure (last vertex == first vertex)
                if i == 0 && j == n - 1 {
                    continue;
                }
                // sub1 = edges from LEAVING V (at i) to ENTERING V (at j)
                // face[i+1] leaves V (from_vertex = verts[i] = V)
                // face[j] enters V (to_vertex = verts[j] = V)
                let sub1: Vec<(usize, usize)> = face[i + 1..=j].to_vec();
                // sub2 = remaining edges from LEAVING V (at j) to ENTERING V (at i)
                // face[j+1] leaves V (from_vertex = verts[j] = V)
                // face[i] enters V (to_vertex = verts[i] = V)
                let sub2: Vec<(usize, usize)> = face[j + 1..]
                    .iter()
                    .chain(face[0..=i].iter())
                    .copied()
                    .collect();
                let mut result = split_face_at_pinch_points(&sub1);
                result.extend(split_face_at_pinch_points(&sub2));
                return result;
            }
        }
    }
    vec![face.to_vec()]
}

/// ---------------------------------------------------------------------------
/// Face labeling: BFS from exterior face toggling interior/exterior
/// ---------------------------------------------------------------------------

fn label_interior_faces(
    edges: &[Line<f64>],
    faces: &[Vec<(usize, usize)>],
) -> Option<HashSet<usize>> {
    let n_faces = faces.len();
    if n_faces == 0 {
        return None;
    }

    let mut edge_to_faces: HashMap<usize, Vec<usize>> = HashMap::new();
    for (fi, face) in faces.iter().enumerate() {
        for &(ei, _) in face {
            edge_to_faces.entry(ei).or_default().push(fi);
        }
    }

    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
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

    let exterior = find_exterior_face(faces, edges)?;

    let mut interior: HashSet<usize> = HashSet::new();
    let mut visited: HashSet<usize> = HashSet::new();
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
    Some(interior)
}

fn find_exterior_face(faces: &[Vec<(usize, usize)>], edges: &[Line<f64>]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (fi, face) in faces.iter().enumerate() {
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for &(ei, _) in face {
            let e = &edges[ei];
            min_x = min_x.min(e.start.x).min(e.end.x);
            max_x = max_x.max(e.start.x).max(e.end.x);
            min_y = min_y.min(e.start.y).min(e.end.y);
            max_y = max_y.max(e.start.y).max(e.end.y);
        }
        let area = (max_x - min_x) * (max_y - min_y);
        if best.is_none() || area > best.unwrap().1 {
            best = Some((fi, area));
        }
    }
    best.map(|(i, _)| i)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_square() {
        let ring = ls(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        let r = repair_ring(&ring);
        assert!(r.is_some());
        assert_eq!(r.unwrap().len(), 1);
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
        let r = repair_ring(&ring);
        assert!(r.is_some(), "bowtie should produce result");
        let rings = r.unwrap();
        assert!(!rings.is_empty(), "bowtie should produce at least one ring");
        for ring in &rings {
            assert!(ring.0.len() >= 4, "ring too short");
            assert_eq!(ring.0.first(), ring.0.last(), "ring not closed");
        }
    }

    #[test]
    fn test_self_touching() {
        let ring = ls(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (5.0, 5.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        let r = repair_ring(&ring);
        assert!(r.is_some());
    }

    #[test]
    fn test_figure_eight() {
        let ring = ls(&[
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]);
        let r = repair_ring(&ring);
        assert!(r.is_some());
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
        let (graph, _) = build_graph(&edges);
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

    fn ls(pairs: &[(f64, f64)]) -> LineString<f64> {
        LineString::new(pairs.iter().map(|&(x, y)| Coord { x, y }).collect())
    }

    fn coords(pairs: &[(f64, f64)]) -> Vec<Coord<f64>> {
        pairs.iter().map(|&(x, y)| Coord { x, y }).collect()
    }
}

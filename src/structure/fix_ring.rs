use rustc_hash::FxHashSet;
use smallvec::SmallVec;

use geo::{Coord, Line, LineString};

use crate::orient::orient2d;
use crate::structure::fix_ring_graph::{
    build_graph, extract_all_faces, label_interior_faces, split_face_at_pinch_points,
};

type SplitPoint = SmallVec<[(f64, Coord<f64>); 2]>;

pub(crate) fn repair_ring(ring: &LineString<f64>) -> Option<Vec<LineString<f64>>> {
    let coords = basic_cleanup(ring)?;
    if coords.len() < 4 {
        return None;
    }
    if !has_self_intersections(&coords) {
        return Some(vec![LineString::new(coords)]);
    }

    if let Some(rings) = fix_self_intersecting(&coords) {
        let cleaned: Vec<LineString<f64>> = rings
            .into_iter()
            .filter_map(|r| basic_cleanup(&r).map(LineString::new))
            .filter(|r| r.0.len() >= 4)
            .collect();
        if cleaned.is_empty() {
            return None;
        }
        return Some(cleaned);
    }
    None
}

pub(crate) fn basic_cleanup(ring: &LineString<f64>) -> Option<Vec<Coord<f64>>> {
    let coords: Vec<_> = ring
        .0
        .iter()
        .copied()
        .filter(|c| c.x.is_finite() && c.y.is_finite())
        .collect();
    if coords.is_empty() {
        return None;
    }
    let mut deduped = remove_consecutive_duplicates(&coords);
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

fn remove_consecutive_duplicates(coords: &[Coord<f64>]) -> Vec<Coord<f64>> {
    let mut result = Vec::with_capacity(coords.len());
    for c in coords {
        if result.last() != Some(c) {
            result.push(*c);
        }
    }
    result
}

pub(crate) fn has_self_intersections(coords: &[Coord<f64>]) -> bool {
    let n = coords.len();
    if n < 4 {
        return false;
    }

    let mut seen: FxHashSet<(u64, u64)> =
        FxHashSet::with_capacity_and_hasher(n, Default::default());
    for c in &coords[..n - 1] {
        let key = (c.x.to_bits(), c.y.to_bits());
        if !seen.insert(key) {
            return true;
        }
    }

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &c in coords {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * coord_scale;

    if n > 500 {
        return super::sweep::has_self_intersections(coords, eps);
    }

    has_self_intersections_bruteforce(coords, eps)
}

fn has_self_intersections_bruteforce(coords: &[Coord<f64>], eps: f64) -> bool {
    let n = coords.len();
    for i in 0..n - 1 {
        for j in i + 2..n - 1 {
            if i == 0 && j == n - 2 {
                continue;
            }
            if check_edge_pair(coords, i, j, eps) {
                return true;
            }
        }
    }
    false
}

#[inline(always)]
pub(crate) fn check_edge_pair(coords: &[Coord<f64>], i: usize, j: usize, eps: f64) -> bool {
    assert!(i + 1 < coords.len() && j + 1 < coords.len());
    let a1 = coords[i];
    let a2 = coords[i + 1];
    let b1 = coords[j];
    let b2 = coords[j + 1];

    let o = crate::simd::orient2d_batch_4_robust(
        &[a1, a1, b1, b1],
        &[a2, a2, b2, b2],
        &[b1, b2, a1, a2],
    );

    if o[0] * o[1] < 0.0 && o[2] * o[3] < 0.0 {
        return true;
    }

    if o[2].abs() <= eps
        && a1 != b1
        && a1 != b2
        && ((b1.x - b2.x).abs() > eps && a1.x > b1.x.min(b2.x) + eps && a1.x < b1.x.max(b2.x) - eps
            || (b1.y - b2.y).abs() > eps
                && a1.y > b1.y.min(b2.y) + eps
                && a1.y < b1.y.max(b2.y) - eps)
    {
        return true;
    }
    if o[3].abs() <= eps
        && a2 != b1
        && a2 != b2
        && ((b1.x - b2.x).abs() > eps && a2.x > b1.x.min(b2.x) + eps && a2.x < b1.x.max(b2.x) - eps
            || (b1.y - b2.y).abs() > eps
                && a2.y > b1.y.min(b2.y) + eps
                && a2.y < b1.y.max(b2.y) - eps)
    {
        return true;
    }
    if o[0].abs() <= eps
        && b1 != a1
        && b1 != a2
        && ((a1.x - a2.x).abs() > eps && b1.x > a1.x.min(a2.x) + eps && b1.x < a1.x.max(a2.x) - eps
            || (a1.y - a2.y).abs() > eps
                && b1.y > a1.y.min(a2.y) + eps
                && b1.y < a1.y.max(a2.y) - eps)
    {
        return true;
    }
    if o[1].abs() <= eps
        && b2 != a1
        && b2 != a2
        && ((a1.x - a2.x).abs() > eps && b2.x > a1.x.min(a2.x) + eps && b2.x < a1.x.max(a2.x) - eps
            || (a1.y - a2.y).abs() > eps
                && b2.y > a1.y.min(a2.y) + eps
                && b2.y < a1.y.max(a2.y) - eps)
    {
        return true;
    }

    if o[0].abs() <= eps && o[1].abs() <= eps && o[2].abs() <= eps && o[3].abs() <= eps {
        let lo_x = a1.x.min(a2.x).max(b1.x.min(b2.x));
        let hi_x = a1.x.max(a2.x).min(b1.x.max(b2.x));
        let lo_y = a1.y.min(a2.y).max(b1.y.min(b2.y));
        let hi_y = a1.y.max(a2.y).min(b1.y.max(b2.y));
        if lo_x + eps < hi_x || lo_y + eps < hi_y {
            return true;
        }
    }

    false
}

/// ---------------------------------------------------------------------------
/// Self-intersecting ring fixer
/// ---------------------------------------------------------------------------
pub(crate) fn fix_self_intersecting(coords: &[Coord<f64>]) -> Option<Vec<LineString<f64>>> {
    let edges = edges_from_coords(coords);
    let noded = split_edges(&edges);
    if noded.is_empty() {
        return None;
    }
    let graph = build_graph(&noded);
    if graph.edges.is_empty() {
        return None;
    }
    if std::env::var("DIAG_FIX_RING").is_ok() {
        eprintln!("\n=== fix_self_intersecting DIAG ===");
        eprintln!("input coords ({}):", coords.len());
        for (i, c) in coords.iter().enumerate() {
            eprintln!("  c{}: ({:.10}, {:.10})", i, c.x, c.y);
        }
        eprintln!("noded edges ({}):", graph.edges.len());
        for (i, &(fi, ti)) in graph.edges.iter().enumerate() {
            eprintln!(
                "  E{}: v{} ({:.6},{:.6}) -> v{} ({:.6},{:.6})",
                i,
                fi,
                graph.verts[fi].x,
                graph.verts[fi].y,
                ti,
                graph.verts[ti].x,
                graph.verts[ti].y
            );
        }
        eprintln!("verts ({}):", graph.verts.len());
        for (i, v) in graph.verts.iter().enumerate() {
            eprintln!("  v{}: ({:.10}, {:.10})", i, v.x, v.y);
        }
    }
    let faces = extract_all_faces(&graph)?;
    if faces.is_empty() {
        return None;
    }
    if std::env::var("DIAG_FIX_RING").is_ok() {
        eprintln!("\nfragments from extract_all_faces ({}):", faces.len());
        for (fi, face) in faces.iter().enumerate() {
            eprintln!("  face {}: {} edges", fi, face.len());
            for (ei, to) in face {
                eprintln!(
                    "    (E{}, to=v{}[{:.4},{:.4}])",
                    ei, to, graph.verts[*to].x, graph.verts[*to].y
                );
            }
        }
    }
    let simple_faces: Vec<Vec<(usize, usize)>> = faces
        .iter()
        .flat_map(|f| split_face_at_pinch_points(f, &graph.edges))
        .filter(|f| f.len() >= 3)
        .collect();
    if simple_faces.is_empty() {
        return None;
    }
    if std::env::var("DIAG_FIX_RING").is_ok() {
        eprintln!("\nsimple_faces after pinch split ({}):", simple_faces.len());
        for (fi, face) in simple_faces.iter().enumerate() {
            eprintln!("  face {}: {} edges", fi, face.len());
            let visited_verts: Vec<usize> = face.iter().map(|&(_, to)| to).collect();
            eprintln!("    verts: {:?}", visited_verts);
            eprintln!("    coords:");
            for (j, (ei, to)) in face.iter().enumerate() {
                eprintln!(
                    "      {}: E{} -> v{} ({:.10}, {:.10})",
                    j, ei, to, graph.verts[*to].x, graph.verts[*to].y
                );
            }
        }
    }
    let interior = label_interior_faces(&noded, &graph.verts, coords, &simple_faces, &graph.edges)?;
    if std::env::var("DIAG_FIX_RING").is_ok() {
        eprintln!(
            "\ninterior faces: {:?}",
            interior.iter().collect::<Vec<_>>()
        );
    }
    let mut result: Vec<LineString<f64>> = Vec::new();
    for &fi in &interior {
        let face = &simple_faces[fi];
        let mut ring_coords: Vec<Coord<f64>> = face
            .iter()
            .map(|&(_, to_idx)| graph.verts[to_idx])
            .collect();
        if ring_coords.len() >= 3 {
            ring_coords.push(ring_coords[0]);
            if std::env::var("DIAG_FIX_RING").is_ok() {
                eprintln!("  interior ring coords ({}):", ring_coords.len());
                let visited: Vec<usize> = face.iter().map(|&(_, to)| to).collect();
                eprintln!("    verts: {:?}", visited);
                for (j, c) in ring_coords.iter().enumerate() {
                    eprintln!("      {}: ({:.10}, {:.10})", j, c.x, c.y);
                }
            }
            result.push(LineString::new(ring_coords));
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

pub(crate) fn edges_from_coords(coords: &[Coord<f64>]) -> Vec<Line<f64>> {
    coords.windows(2).map(|w| Line::new(w[0], w[1])).collect()
}

/// ---------------------------------------------------------------------------
/// Edge splitting at intersection points
/// ---------------------------------------------------------------------------
pub(crate) fn split_edges(edges: &[Line<f64>]) -> Vec<Line<f64>> {
    let n = edges.len();
    let mut split_points: Vec<SplitPoint> = vec![SmallVec::new(); n];

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for e in edges {
        min_x = min_x.min(e.start.x).min(e.end.x);
        max_x = max_x.max(e.start.x).max(e.end.x);
        min_y = min_y.min(e.start.y).min(e.end.y);
        max_y = max_y.max(e.start.y).max(e.end.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * coord_scale;

    if n > 500 {
        split_edges_grid(edges, &mut split_points, eps);
    } else {
        split_edges_bruteforce(edges, &mut split_points, eps);
    }

    let eps_param = 1e-14;
    let mut result = Vec::new();
    for i in 0..n {
        let e = edges[i];
        let mut pts = std::mem::take(&mut split_points[i]);
        pts.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap());
        pts.dedup_by(|(a, _), (b, _)| (*a - *b).abs() < eps_param);
        let mut prev_pt = e.start;
        for &(_, pt) in &pts {
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

fn split_edges_bruteforce(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let n = edges.len();
    for i in 0..n {
        for j in (i + 2)..n {
            if i + 1 == j && edges[i].end == edges[j].start {
                continue;
            }
            if i == 0 && j == n - 1 && edges[i].start == edges[j].end {
                continue;
            }
            if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps)
                && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
            {
                let pi = lerp(edges[i], ti);
                let pj = lerp(edges[j], tj);
                let pt = Coord {
                    x: (pi.x + pj.x) * 0.5,
                    y: (pi.y + pj.y) * 0.5,
                };
                if ti > eps && ti < 1.0 - eps {
                    split_points[i].push((ti, pt));
                }
                if tj > eps && tj < 1.0 - eps {
                    split_points[j].push((tj, pt));
                }
            }
        }
    }
}

fn split_edges_grid(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let n = edges.len();
    let grid = build_edge_grid(edges);

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        use rayon::prelude::*;
        let cell_results: Vec<Vec<(usize, f64, Coord<f64>)>> = grid
            .par_iter()
            .map(|cell| {
                let mut hits = Vec::new();
                if cell.len() < 2 {
                    return hits;
                }
                let mut sorted = cell.clone();
                sorted.sort_unstable();
                for (ii, &ei) in sorted.iter().enumerate() {
                    for &ej in sorted.iter().skip(ii + 1) {
                        if ei.abs_diff(ej) <= 1 || (ei == 0 && ej == n - 1) {
                            continue;
                        }
                        if let Some((ti, tj)) = intersect_param(&edges[ei], &edges[ej], eps)
                            && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
                        {
                            let pi = lerp(edges[ei], ti);
                            let pj = lerp(edges[ej], tj);
                            let pt = Coord {
                                x: (pi.x + pj.x) * 0.5,
                                y: (pi.y + pj.y) * 0.5,
                            };
                            if ti > eps && ti < 1.0 - eps {
                                hits.push((ei, ti, pt));
                            }
                            if tj > eps && tj < 1.0 - eps {
                                hits.push((ej, tj, pt));
                            }
                        }
                    }
                }
                hits
            })
            .collect();
        for hits in cell_results {
            for (ei, t, pt) in hits {
                split_points[ei].push((t, pt));
            }
        }
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        let mut checked: FxHashSet<(usize, usize)> = FxHashSet::default();
        for cell in &grid {
            if cell.len() < 2 {
                continue;
            }
            let mut sorted = cell.clone();
            sorted.sort_unstable();
            for (ii, &ei) in sorted.iter().enumerate() {
                for &ej in sorted.iter().skip(ii + 1) {
                    if !checked.insert((ei, ej)) {
                        continue;
                    }
                    if ei.abs_diff(ej) <= 1 || (ei == 0 && ej == n - 1) {
                        continue;
                    }
                    if let Some((ti, tj)) = intersect_param(&edges[ei], &edges[ej], eps)
                        && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
                    {
                        let pi = lerp(edges[ei], ti);
                        let pj = lerp(edges[ej], tj);
                        let pt = Coord {
                            x: (pi.x + pj.x) * 0.5,
                            y: (pi.y + pj.y) * 0.5,
                        };
                        if ti > eps && ti < 1.0 - eps {
                            split_points[ei].push((ti, pt));
                        }
                        if tj > eps && tj < 1.0 - eps {
                            split_points[ej].push((tj, pt));
                        }
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
    let grid_dim = grid_dim.clamp(4, 256);
    let cell_w = dx / grid_dim as f64;
    let cell_h = dy / grid_dim as f64;
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); grid_dim * grid_dim];

    for (ei, e) in edges.iter().enumerate() {
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

#[inline]
fn intersect_param(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
    let denom = (e1.end.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e1.end.y - e1.start.y) * (e2.end.x - e2.start.x);
    if denom.abs() < eps {
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

#[inline]
fn intersect_param_collinear(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
    let o1 = orient2d(e1.start, e1.end, e2.start);
    let o2 = orient2d(e1.start, e1.end, e2.end);
    let o3 = orient2d(e2.start, e2.end, e1.start);
    let o4 = orient2d(e2.start, e2.end, e1.end);
    if o1.abs() > eps || o2.abs() > eps || o3.abs() > eps || o4.abs() > eps {
        return None;
    }

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
        let e2_dot = |c: Coord<f64>| -> f64 {
            let dx2 = e2.end.x - e2.start.x;
            let dy2 = e2.end.y - e2.start.y;
            let len2_2 = dx2 * dx2 + dy2 * dy2;
            if len2_2 < eps {
                return 0.0;
            }
            ((c.x - e2.start.x) * dx2 + (c.y - e2.start.y) * dy2) / len2_2
        };

        let mid_x = e1.start.x + lo * dx;
        let mid_y = e1.start.y + lo * dy;
        let mid = Coord { x: mid_x, y: mid_y };

        let t_param = lo;
        let u_param = e2_dot(mid).clamp(0.0, 1.0);

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

#[inline(always)]
fn lerp(e: Line<f64>, t: f64) -> Coord<f64> {
    Coord {
        x: e.start.x + t * (e.end.x - e.start.x),
        y: e.start.y + t * (e.end.y - e.start.y),
    }
}

#[inline(always)]
fn dist2(a: Coord<f64>, b: Coord<f64>) -> f64 {
    (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
}

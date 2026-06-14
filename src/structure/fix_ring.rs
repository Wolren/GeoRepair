use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::collections::VecDeque;

use geo::{Coord, Line, LineString};

use crate::orient::orient2d;

pub(crate) fn repair_ring(ring: &LineString<f64>) -> Option<Vec<LineString<f64>>> {
    let coords = basic_cleanup(ring)?;
    if coords.len() < 4 {
        return None;
    }
    if !has_self_intersections(&coords) {
        return Some(vec![LineString::new(coords)]);
    }

    // Planar graph face extraction (O(n) face splitting via Vec index lookup)
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

    // Check for duplicate non-consecutive vertices (pinch points) via FxHashSet
    // These trigger the planar graph which handles them via split_face_at_pinch_points.
    let mut seen: FxHashSet<(u64, u64)> =
        FxHashSet::with_capacity_and_hasher(n, Default::default());
    for i in 0..n - 1 {
        let key = (coords[i].x.to_bits(), coords[i].y.to_bits());
        if !seen.insert(key) {
            return true;
        }
    }

    // Compute coordinate-scale-relative epsilon for edge checks at large scales
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &c in coords {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * coord_scale;

    // Sweep-line via interval tree — O((n + k) log n)
    if n > 500 {
        return super::sweep::has_self_intersections(coords, eps);
    }

    has_self_intersections_bruteforce(coords, eps)
}

/// O(n²) brute force for small rings
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

/// Check a single pair of edges for any type of intersection.
/// Uses a single batch of 4 orient2d calls (SIMD) for ALL sub-checks.
#[inline(always)]
pub(crate) fn check_edge_pair(coords: &[Coord<f64>], i: usize, j: usize, eps: f64) -> bool {
    assert!(i + 1 < coords.len() && j + 1 < coords.len());
    let a1 = coords[i];
    let a2 = coords[i + 1];
    let b1 = coords[j];
    let b2 = coords[j + 1];

    // Single batch of 4 orient2d:
    // o[0] = orient2d(a1, a2, b1)
    // o[1] = orient2d(a1, a2, b2)
    // o[2] = orient2d(b1, b2, a1)
    // o[3] = orient2d(b1, b2, a2)
    let o = crate::simd::orient2d_batch_4_robust(
        &[a1, a1, b1, b1],
        &[a2, a2, b2, b2],
        &[b1, b2, a1, a2],
    );

    // edges_cross_proper — strict sign product: zero = shared endpoint, not a crossing
    if o[0] * o[1] < 0.0 && o[2] * o[3] < 0.0 {
        return true;
    }

    // vertex_on_edge(a1 on b1-b2): orient2d(b1, b2, a1) = o[2]
    // vertex_on_edge(a2 on b1-b2): orient2d(b1, b2, a2) = o[3]
    // vertex_on_edge(b1 on a1-a2): orient2d(a1, a2, b1) = o[0]
    // vertex_on_edge(b2 on a1-a2): orient2d(a1, a2, b2) = o[1]
    if o[2].abs() <= eps && a1 != b1 && a1 != b2 {
        if (b1.x - b2.x).abs() > eps && a1.x > b1.x.min(b2.x) + eps && a1.x < b1.x.max(b2.x) - eps
            || (b1.y - b2.y).abs() > eps
                && a1.y > b1.y.min(b2.y) + eps
                && a1.y < b1.y.max(b2.y) - eps
        {
            return true;
        }
    }
    if o[3].abs() <= eps && a2 != b1 && a2 != b2 {
        if (b1.x - b2.x).abs() > eps && a2.x > b1.x.min(b2.x) + eps && a2.x < b1.x.max(b2.x) - eps
            || (b1.y - b2.y).abs() > eps
                && a2.y > b1.y.min(b2.y) + eps
                && a2.y < b1.y.max(b2.y) - eps
        {
            return true;
        }
    }
    if o[0].abs() <= eps && b1 != a1 && b1 != a2 {
        if (a1.x - a2.x).abs() > eps && b1.x > a1.x.min(a2.x) + eps && b1.x < a1.x.max(a2.x) - eps
            || (a1.y - a2.y).abs() > eps
                && b1.y > a1.y.min(a2.y) + eps
                && b1.y < a1.y.max(a2.y) - eps
        {
            return true;
        }
    }
    if o[1].abs() <= eps && b2 != a1 && b2 != a2 {
        if (a1.x - a2.x).abs() > eps && b2.x > a1.x.min(a2.x) + eps && b2.x < a1.x.max(a2.x) - eps
            || (a1.y - a2.y).abs() > eps
                && b2.y > a1.y.min(a2.y) + eps
                && b2.y < a1.y.max(a2.y) - eps
        {
            return true;
        }
    }

    // edges_collinear_overlap: all 4 orient2d are zero
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
/// Self-intersecting ring fixer
/// ---------------------------------------------------------------------------
/// Self-intersecting ring fixer
/// ---------------------------------------------------------------------------

fn fix_self_intersecting(coords: &[Coord<f64>]) -> Option<Vec<LineString<f64>>> {
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
    // Split faces at pinch points (repeated vertices)
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

fn edges_from_coords(coords: &[Coord<f64>]) -> Vec<Line<f64>> {
    coords.windows(2).map(|w| Line::new(w[0], w[1])).collect()
}

/// ---------------------------------------------------------------------------
/// Edge splitting at intersection points
/// ---------------------------------------------------------------------------

fn split_edges(edges: &[Line<f64>]) -> Vec<Line<f64>> {
    let n = edges.len();
    let mut split_points: Vec<SmallVec<[(f64, Coord<f64>); 2]>> = vec![SmallVec::new(); n];

    // Scale epsilon with coordinate magnitude so that near-parallel detection
    // works correctly for large coordinates (e.g. UTM at 5 million scale).
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

fn split_edges_bruteforce(
    edges: &[Line<f64>],
    split_points: &mut [SmallVec<[(f64, Coord<f64>); 2]>],
    eps: f64,
) {
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
                // Compute split point ONCE from BOTH edges, then average.
                // This ensures both edges are split at the SAME coordinate,
                // preventing T-junctions in the planar graph.
                if (ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps) {
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
}

fn split_edges_grid(
    edges: &[Line<f64>],
    split_points: &mut [SmallVec<[(f64, Coord<f64>); 2]>],
    eps: f64,
) {
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
                for ii in 0..sorted.len() {
                    let ei = sorted[ii];
                    for jj in (ii + 1)..sorted.len() {
                        let ej = sorted[jj];
                        if ei.abs_diff(ej) <= 1 || (ei == 0 && ej == n - 1) {
                            continue;
                        }
                        if let Some((ti, tj)) = intersect_param(&edges[ei], &edges[ej], eps) {
                            if (ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps) {
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
                        if (ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps) {
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

#[inline]
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
#[inline]
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

/// ---------------------------------------------------------------------------
/// Graph construction
/// ---------------------------------------------------------------------------

const SNAP_SCALE: f64 = 1e8;

#[inline(always)]
fn snap_key(c: Coord<f64>) -> (i64, i64) {
    // Guard against overflow: if product exceeds i64 range, clamp
    let sx = c.x * SNAP_SCALE;
    let sy = c.y * SNAP_SCALE;
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
        x: key.0 as f64 / SNAP_SCALE,
        y: key.1 as f64 / SNAP_SCALE,
    }
}

struct Graph {
    verts: Vec<Coord<f64>>,
    edges: Vec<(usize, usize)>,
    sorted_adj: Vec<SmallVec<[(usize, usize); 4]>>,
}

fn build_graph(lines: &[Line<f64>]) -> Graph {
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

        // Find next edge with smallest CCW turn from incoming direction.
        // Allow the start edge as a candidate (it may be used).
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

        // Skip used edges UNLESS this is the start edge (allows cycle closure)
        if used && e_idx != start_ei {
            continue;
        }
        // Skip if edge was traversed in ANY direction within this walk
        if used_any_dir[e_idx] && e_idx != start_ei {
            continue;
        }
        // For the start edge, only allow traversal in the original direction
        // (prevents stealing the edge from the adjacent face)
        if e_idx == start_ei && is_forward != start_is_forward {
            continue;
        }

        let dest = if is_forward { to_idx } else { from_idx };
        let out_angle = (graph.verts[dest].y - graph.verts[v_idx].y)
            .atan2(graph.verts[dest].x - graph.verts[v_idx].x);

        let mut turn = out_angle - incoming_angle;
        // For near-zero negative turns (floating-point noise from nearly colinear
        // edges), clamp to zero instead of wrapping to ~2π. Without this, the
        // smallest CCW turn picks the wrong outgoing edge, creating self-intersecting
        // face boundaries.
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

fn split_face_at_pinch_points(
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

    // Compute the start vertex: the "from" of the first edge's traversal
    let (first_from, first_to) = edges[face[0].0];
    let start_vert = if first_to == face[0].1 {
        first_from
    } else {
        first_to
    };

    // Try to close incomplete faces first (before n < 3 check so 2-edge stubs get closed)
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
    // Virtual position 0 = start vertex
    first_seen[start_vert] = Some(0);

    for j in 0..n {
        let v = to_verts[j];
        if let Some(i) = first_seen[v] {
            if i == 0 && j == n - 1 {
                continue;
            }
            if i == 0 {
                // Repeat of start vertex — split here
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

/// Compute the winding number of the input ring at a point guaranteed to be
/// inside the given face (using edge-offset from the first edge, since the
/// simple centroid may lie outside non-convex faces).
fn face_winding(
    face: &[(usize, usize)],
    verts: &[Coord<f64>],
    graph_edges: &[(usize, usize)],
    input_ring: &[Coord<f64>],
) -> i32 {
    use crate::orient::orient2d;

    // Interior point: midpoint of first edge, offset left of traversal direction
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
    // Left perpendicular (face interior for CCW traversal)
    let mx = (start_coord.x + end_coord.x) * 0.5;
    let my = (start_coord.y + end_coord.y) * 0.5;
    // 1e-7 offset: large enough to overcome snap quantization noise (SNAP_SCALE=1e8)
    // and ensure orient2d sign is stable near input edges that coincide with graph edges.
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

/// Label interior faces using BFS with winding-number verification.
/// BFS toggles parity across shared edges (works for 99% of cases).
/// Winding-number at face edge-offset point catches any mislabeled faces.
fn label_interior_faces(
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

    // Build adjacency from shared edges
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

    // Find exterior face (largest bbox area)
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

    // BFS labeling
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

    // Verify each labeled interior face via winding number on input ring.
    // Uses non-zero winding rule: wn == 0 → outside (exclude).
    // Uses edge-offset test point (guaranteed inside the face) rather than
    // centroid, which can lie outside non-convex faces.
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

    // Faces missed by BFS (share only vertices, not edges) — verify
    // unvisited faces via winding number on the input ring.
    // Skipped when BFS reached all faces AND the exterior choice was
    // correct (large-bbox heuristic can fail for self-intersecting rings).
    if visited.len() < n_faces || {
        // Verify BFS exterior choice — if edge-offset point is inside the input
        // ring, labels are flipped and we need the second pass.
        let ext_face = &faces[exterior];
        let wn = face_winding(ext_face, verts, graph_edges, input_ring);
        wn != 0 // non-zero → BFS exterior is actually inside → labels flipped
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
            // Check if face boundary would be self-intersecting
            let mut ring: Vec<Coord<f64>> = face.iter().map(|&(_, to)| graph.verts[to]).collect();
            if ring.len() >= 3 {
                ring.push(ring[0]);
            }
            let check_si = has_self_intersections(&ring);
            eprintln!("    self-intersecting boundary: {}", check_si);
        }

        let simple_faces: Vec<Vec<(usize, usize)>> = faces
            .iter()
            .flat_map(|f| split_face_at_pinch_points(f))
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

        let interior = label_interior_faces(&noded, &graph.verts, &coords, &simple_faces).unwrap();
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

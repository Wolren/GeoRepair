pub mod classify;
pub mod fix_ring;
pub mod merge;
pub mod subtract;

use geo::{Coord, Geometry, LineString, LinesIter, MultiPolygon, Polygon, Winding};

use crate::config::MakeValidConfig;

pub(crate) fn fix_polygon(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    // Fast path: valid polygons can return immediately. Use a total-verts limit
    // to avoid the mono-chain has_no_intersections O(n²) on very large rings.
    #[cfg(feature = "arrange")]
    {
        let total_verts: usize =
            poly.exterior().0.len() + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
        if total_verts > 0
            && total_verts <= 50000
            && poly.exterior().0.len() >= 4
            && crate::arrange::poly_has_basic_form(poly)
        {
            let lines: Vec<_> = poly.lines_iter().collect();
            if !lines.is_empty()
                && crate::arrange::prep::has_no_intersections(&lines)
                && crate::arrange::holes_are_valid(poly)
            {
                return Some(Geometry::Polygon(poly.clone()));
            }
        }
    }

    // Fix the shell ring — may produce multiple rings if self-intersecting
    let shell_rings = match fix_ring::repair_ring(poly.exterior()) {
        Some(rings) => rings,
        None => {
            #[cfg(feature = "arrange")]
            if !poly.exterior().0.is_empty() {
                return Some(crate::arrange::fix_polygon(poly, config));
            }
            return None;
        }
    };
    if shell_rings.is_empty() {
        return None;
    }
    let valid_shells: Vec<LineString<f64>> =
        shell_rings.into_iter().filter(|s| s.0.len() >= 4).collect();
    if valid_shells.is_empty() {
        return None;
    }

    // Fix holes — each hole may produce multiple rings (parallel via rayon)
    #[cfg(feature = "parallel")]
    let hole_rings_cw: Vec<LineString<f64>> = {
        use rayon::prelude::*;
        let mut hole_results: Vec<Vec<LineString<f64>>> = poly
            .interiors()
            .par_iter()
            .map(|h| fix_ring::repair_ring(h).unwrap_or_else(|| vec![h.clone()]))
            .collect();
        hole_results
            .iter_mut()
            .flat_map(|rings| rings.drain(..))
            .map(ensure_cw)
            .collect()
    };
    #[cfg(not(feature = "parallel"))]
    let hole_rings_cw: Vec<LineString<f64>> = {
        let mut hole_rings: Vec<LineString<f64>> = Vec::new();
        for h in poly.interiors() {
            if let Some(rings) = fix_ring::repair_ring(h) {
                hole_rings.extend(rings);
            } else {
                hole_rings.push(ensure_cw(h.clone()));
            }
        }
        hole_rings.into_iter().map(ensure_cw).collect()
    };

    // For each valid shell ring, classify and subtract holes
    let mut result_polys: Vec<Polygon<f64>> = Vec::new();
    for shell in valid_shells {
        let shell_poly = Polygon::new(ensure_ccw(shell), Vec::new());

        let (inner_holes, outer_holes) =
            classify::classify_holes(shell_poly.exterior(), &hole_rings_cw);

        let (to_subtract, islands) = resolve_nesting(&inner_holes);

        let inner_polys: Vec<Polygon<f64>> = to_subtract
            .into_iter()
            .map(|h| Polygon::new(h, Vec::new()))
            .collect();

        if let Some(current) = subtract::subtract_holes(&shell_poly, &inner_polys) {
            result_polys.push(current);
        }

        result_polys.extend(islands);

        for hole in outer_holes {
            result_polys.push(Polygon::new(hole, Vec::new()));
        }
    }

    if result_polys.is_empty() {
        #[cfg(not(feature = "arrange"))]
        return None;
        #[cfg(feature = "arrange")]
        return Some(Geometry::MultiPolygon(MultiPolygon::new(Vec::new())));
    }

    let result = if result_polys.len() == 1 {
        Geometry::Polygon(result_polys.into_iter().next().unwrap())
    } else {
        Geometry::MultiPolygon(MultiPolygon::new(merge::merge_shells(result_polys).0))
    };

    Some(enforce_winding(result))
}

/// Winding-number point-in-ring test (exclusive of boundary).
/// Delegates to SIMD-accelerated implementation.
fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    crate::simd::point_in_ring_exclusive(pt, ring)
}

/// Resolve hole-hole nesting among inner holes of a shell.
///
/// Returns:
/// - `to_subtract`: holes at containment depth 1 (directly inside the shell).
///   These are subtracted from the shell via boolean difference.
/// - `islands`: holes at depth 2+ become separate polygons, with their own
///   sub-holes (depth 3) as interior rings. Depth alternates: even depths are
///   separate polygons (islands/positive space), odd depths are holes (negative space).
fn resolve_nesting(holes: &[LineString<f64>]) -> (Vec<LineString<f64>>, Vec<Polygon<f64>>) {
    if holes.len() <= 1 {
        return (holes.to_vec(), Vec::new());
    }

    // Build parent relationship: hole[j] is inside hole[i] → parent_of[j] = Some(i)
    let n = holes.len();

    #[cfg(feature = "parallel")]
    let parent_of: Vec<Option<usize>> = {
        use rayon::prelude::*;
        (0..n)
            .into_par_iter()
            .map(|j| {
                let pt = holes[j].0.first().copied()?;
                for i in 0..n {
                    if i == j {
                        continue;
                    }
                    if point_in_ring_exclusive(pt, &holes[i].0) {
                        return Some(i);
                    }
                }

                None
            })
            .collect()
    };
    #[cfg(not(feature = "parallel"))]
    let mut parent_of: Vec<Option<usize>> = {
        let mut p = vec![None; n];
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                if p[j].is_some() {
                    continue;
                }
                if !holes[j].0.is_empty() && point_in_ring_exclusive(holes[j].0[0], &holes[i].0) {
                    p[j] = Some(i);
                }
            }
        }
        p
    };

    // Compute containment depth for each hole via topological sort
    #[cfg(feature = "parallel")]
    let mut depth: Vec<usize> = (0..n)
        .map(|i| if parent_of[i].is_none() { 1 } else { 0 })
        .collect();
    #[cfg(not(feature = "parallel"))]
    let mut depth = vec![0usize; n];
    #[cfg(not(feature = "parallel"))]
    for i in 0..n {
        if parent_of[i].is_none() {
            depth[i] = 1;
        }
    }
    // Propagate depths (bounded loop: at most n iterations)
    for _ in 0..n {
        for i in 0..n {
            if let Some(p) = parent_of[i] {
                if depth[p] > 0 {
                    depth[i] = depth[p] + 1;
                }
            }
        }
    }

    // Group holes by depth parity:
    // even depth (2, 4, ...): separate polygons (islands)
    // odd depth (1, 3, ...): subtract-from-parent (holes/voids)
    let mut subtract = Vec::new();
    let mut island_indices = Vec::new();
    for i in 0..n {
        if depth[i] == 0 {
            // Unreachable (shouldn't happen), treat as top-level hole
            subtract.push(i);
        } else if depth[i] % 2 == 1 {
            subtract.push(i);
        } else {
            island_indices.push(i);
        }
    }

    // For depth-2+ holes (islands), assign depth-3+ children as interior rings
    // Build island polygons with proper sub-hole nesting
    let mut islands: Vec<Polygon<f64>> = Vec::new();
    for &ii in &island_indices {
        let children: Vec<LineString<f64>> = (0..n)
            .filter(|&j| parent_of[j] == Some(ii) && depth[j] > depth[ii] && depth[j] % 2 == 1)
            .map(|j| holes[j].clone())
            .collect();
        islands.push(Polygon::new(holes[ii].clone(), children));
    }

    (
        subtract.into_iter().map(|i| holes[i].clone()).collect(),
        islands,
    )
}

fn ensure_ccw(mut ring: LineString<f64>) -> LineString<f64> {
    #[cfg(feature = "simd")]
    let ccw = crate::simd::is_ring_ccw_simd(&ring.0);
    #[cfg(not(feature = "simd"))]
    let ccw = ring.winding_order() == Some(geo::winding_order::WindingOrder::CounterClockwise);
    if !ccw {
        ring.make_ccw_winding();
    }
    ring
}

fn ensure_cw(mut ring: LineString<f64>) -> LineString<f64> {
    if ring.winding_order() != Some(geo::winding_order::WindingOrder::Clockwise) {
        ring.make_cw_winding();
    }
    ring
}

fn enforce_winding(g: Geometry<f64>) -> Geometry<f64> {
    fn fix_ccw(r: LineString<f64>) -> LineString<f64> {
        if r.winding_order() == Some(geo::winding_order::WindingOrder::CounterClockwise) {
            r
        } else {
            let mut c = r;
            c.make_ccw_winding();
            c
        }
    }
    fn fix_cw(r: LineString<f64>) -> LineString<f64> {
        if r.winding_order() == Some(geo::winding_order::WindingOrder::Clockwise) {
            r
        } else {
            let mut c = r;
            c.make_cw_winding();
            c
        }
    }

    match g {
        Geometry::Polygon(p) => {
            let ext = fix_ccw(p.exterior().clone());
            let holes: Vec<_> = p.interiors().iter().map(|h| fix_cw(h.clone())).collect();
            Geometry::Polygon(Polygon::new(ext, holes))
        }
        Geometry::MultiPolygon(mp) => {
            let polys: Vec<_> = mp
                .0
                .into_iter()
                .map(|p| {
                    let ext = fix_ccw(p.exterior().clone());
                    let holes: Vec<_> = p.interiors().iter().map(|h| fix_cw(h.clone())).collect();
                    Polygon::new(ext, holes)
                })
                .collect();
            Geometry::MultiPolygon(MultiPolygon::new(polys))
        }
        other => other,
    }
}

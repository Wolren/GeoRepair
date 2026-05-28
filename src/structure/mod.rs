pub mod classify;
pub mod fix_ring;
pub mod merge;
pub mod subtract;

use geo::validation::Validation;
use geo::{Coord, Geometry, LineString, LinesIter, MultiPolygon, Polygon, Winding};

use crate::config::MakeValidConfig;

pub(crate) fn fix_polygon(poly: &Polygon<f64>, config: &MakeValidConfig) -> Option<Geometry<f64>> {
    // For very large polygons, skip to CDT arrange directly — the structure method's
    // planar graph and OGC validation are O(n²) and catastrophic above ~30K verts.
    #[cfg(feature = "arrange")]
    {
        let total_verts: usize =
            poly.exterior().0.len() + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
        if total_verts > 30000 {
            return Some(crate::arrange::fix_polygon(poly, config));
        }
    }

    // Fast path: valid polygons with no intersections can return immediately
    #[cfg(feature = "arrange")]
    if poly.exterior().0.len() >= 4 && crate::arrange::poly_has_basic_form(poly) {
        let lines: Vec<_> = poly.lines_iter().collect();
        if !lines.is_empty()
            && crate::arrange::prep::has_no_intersections(&lines)
            && crate::arrange::holes_are_valid(poly)
        {
            return Some(Geometry::Polygon(poly.clone()));
        }
    }

    // Fix the shell ring — may produce multiple rings if self-intersecting
    let shell_rings = match fix_ring::repair_ring(poly.exterior()) {
        Some(rings) => rings,
        None => {
            // Ring too large or too degenerate for planar graph — fall back to CDT
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

    // Handle collapsed shell (all rings degenerate)
    let valid_shells: Vec<LineString<f64>> =
        shell_rings.into_iter().filter(|s| s.0.len() >= 4).collect();
    if valid_shells.is_empty() {
        return None;
    }

    // Fix holes — each hole may produce multiple rings
    let mut hole_rings: Vec<LineString<f64>> = Vec::new();
    for h in poly.interiors() {
        if let Some(rings) = fix_ring::repair_ring(h) {
            hole_rings.extend(rings);
        }
    }
    let hole_rings_cw: Vec<LineString<f64>> = hole_rings.into_iter().map(ensure_cw).collect();

    // For each valid shell ring, classify and subtract holes
    let mut result_polys: Vec<Polygon<f64>> = Vec::new();
    for shell in valid_shells {
        let shell_poly = Polygon::new(ensure_ccw(shell), Vec::new());

        let (inner_holes, outer_holes) =
            classify::classify_holes(shell_poly.exterior(), &hole_rings_cw);

        // Resolve hole-hole nesting: build containment tree so that nested holes
        // become separate polygons (islands in voids) instead of being lost in subtraction.
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

    // Validate output — if the planar graph fix produced an invalid or empty result,
    // fall back to arrange (CDT) for robustness
    #[cfg(feature = "arrange")]
    {
        if result_polys.is_empty() {
            return Some(crate::arrange::fix_polygon(poly, config));
        }
        let validated = validate_and_fallback(&result_polys, poly, config);
        if let Some(g) = validated {
            return Some(g);
        }
    }

    #[cfg(not(feature = "arrange"))]
    if result_polys.is_empty() {
        return None;
    }

    let result = if result_polys.len() == 1 {
        Geometry::Polygon(result_polys.into_iter().next().unwrap())
    } else {
        Geometry::MultiPolygon(MultiPolygon::new(merge::merge_shells(result_polys).0))
    };

    Some(result)
}

#[cfg(feature = "arrange")]
fn validate_and_fallback(
    result_polys: &[Polygon<f64>],
    original: &Polygon<f64>,
    config: &MakeValidConfig,
) -> Option<Geometry<f64>> {
    // Check each polygon in the result for validity
    for p in result_polys {
        if p.check_validation().is_err() {
            return Some(crate::arrange::fix_polygon(original, config));
        }
    }
    None
}

/// Winding-number point-in-ring test (exclusive of boundary).
fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    let n = ring.len();
    let mut wn = 0i32;
    for i in 0..n - 1 {
        let p1 = ring[i];
        let p2 = ring[i + 1];
        if p1.y <= pt.y {
            if p2.y > pt.y && orient2d(p1, p2, pt) > 0.0 {
                wn += 1;
            }
        } else if p2.y <= pt.y && orient2d(p1, p2, pt) < 0.0 {
            wn -= 1;
        }
    }
    wn != 0
}

/// Robust 2D orientation predicate.
fn orient2d(a: Coord<f64>, b: Coord<f64>, c: Coord<f64>) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
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
    let mut parent_of: Vec<Option<usize>> = vec![None; n];

    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            if parent_of[j].is_some() {
                continue;
            }
            // hole[i] contains hole[j] if any point of j is inside i
            if contains_hole(&holes[i], &holes[j]) {
                parent_of[j] = Some(i);
            }
        }
    }

    // Compute containment depth for each hole via topological sort
    let mut depth = vec![0usize; n];
    for i in 0..n {
        if parent_of[i].is_none() {
            depth[i] = 1; // direct child of shell
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

/// Check if hole `outer` contains hole `inner`: any point of `inner` is inside `outer`.
fn contains_hole(outer: &LineString<f64>, inner: &LineString<f64>) -> bool {
    if inner.0.is_empty() {
        return false;
    }
    point_in_ring_exclusive(inner.0[0], &outer.0)
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

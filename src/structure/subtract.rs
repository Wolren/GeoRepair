use alloc::vec::Vec;
use geo::{Area, Coord, LinesIter, MultiPolygon, Polygon};

/// Subtract holes from a shell, producing OGC-valid polygon components.
///
/// Strategy:
/// 1. Boolean difference (fast for clean nesting)
/// 2. If the boolean RESULT looks topologically bad (bbox-overlapping holes
///    inside one component → DisconnectedInteriorRing risk), BuildArea on
///    noded shell+hole edges as a fallback.
///
/// Triggering on the *result* (not input heuristics) keeps this fast: the
/// boolean op already handles the common cases (including hourglass
/// diamond-touch holes, which split the shell into multiple components).
/// Input-side heuristics like "hole vertex on shell boundary" fire on
/// ordinary real-world data and forced full noding + polygonize on nearly
/// every polygon — a ~50-100x regression (305s vs 5.5s on 2298 invalid polys).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn subtract_holes(shell: &Polygon<f64>, holes: &[Polygon<f64>]) -> MultiPolygon<f64> {
    if holes.is_empty() {
        return MultiPolygon::new(vec![shell.clone()]);
    }

    // Boolean difference preserves ALL result components — a hole that
    // touches the shell in 2+ places splits the shell into multiple
    // polygons (the classic "hourglass" hole pattern).
    let result = match crate::arrange::boolean_difference_catch(shell, holes) {
        Some(mp) => mp,
        // i_overlay panic (is_fill_top assertion on degenerate input) — the
        // panic must not kill the rayon batch. Route to the BuildArea
        // fallback below, which operates on noded edges and never panics.
        None => {
            if let Some(mp) = build_area_shell_holes(shell, holes) {
                return mp;
            }
            let mut comps: Vec<Polygon<f64>> = vec![shell.clone()];
            comps.extend(holes.iter().cloned());
            if let Some(mp) = polygonize_mp(&MultiPolygon::new(comps)) {
                return mp;
            }
            return MultiPolygon::new(vec![shell.clone()]);
        }
    };

    let eps = 1e-15;
    let valid: Vec<Polygon<f64>> = result
        .0
        .into_iter()
        .filter(|p| p.unsigned_area() > eps)
        .collect();
    let result = MultiPolygon::new(valid);

    // Touching/overlapping holes in the boolean RESULT → DisconnectedInteriorRing.
    // Rebuild via BuildArea on original shell+hole edges, but ONLY when the
    // result actually has collinear edge-sharing between rings (positive-length
    // overlap — the real DIR trigger). Bbox-overlap alone is a terrible signal:
    // adjacent holes in ordinary real-world polygons have overlapping bboxes
    // (the WIP's bbox heuristic forced noding+polygonize on nearly every poly —
    // 305s vs 5.5s on 2298 invalid polys).
    if needs_build_area_holes(&result) {
        if let Some(mp) = build_area_shell_holes(shell, holes) {
            return mp;
        }
        if let Some(mp) = polygonize_mp(&result) {
            return mp;
        }
    }

    result
}

fn needs_build_area_holes(mp: &MultiPolygon<f64>) -> bool {
    for p in &mp.0 {
        let holes: Vec<&[Coord<f64>]> = p.interiors().iter().map(|h| h.0.as_slice()).collect();
        // Hole-hole collinear edge share (positive-length overlap) → DIR.
        for i in 0..holes.len() {
            for j in (i + 1)..holes.len() {
                if crate::util::rings_share_collinear_edge_quantized(holes[i], holes[j]) {
                    return true;
                }
            }
        }
        // Hole-shell collinear edge share → DIR. Only cheap when the shell is
        // small; big shells (10k+ verts) make the pair scan prohibitive and
        // boolean difference handles those well anyway.
        let shell = p.exterior().0.as_slice();
        if shell.len() <= 64 {
            for h in &holes {
                if crate::util::rings_share_collinear_edge_quantized(h, shell) {
                    return true;
                }
            }
        }
    }
    false
}

/// True if two rings share a collinear edge segment with positive-length overlap.
/// Vertex-only touches (hourglass patterns) return false — boolean difference
/// handles those correctly by splitting components.
fn build_area_shell_holes(
    shell: &Polygon<f64>,
    holes: &[Polygon<f64>],
) -> Option<MultiPolygon<f64>> {
    let mut lines: Vec<geo::Line<f64>> = shell.lines_iter().collect();
    for h in holes {
        lines.extend(h.lines_iter());
    }
    if lines.is_empty() {
        return None;
    }
    let noded = crate::arrange::prep::prepare_lines(lines).ok()?;
    if noded.lines.is_empty() {
        return None;
    }
    let faces = crate::structure::polygonizer::polygonize(&noded.lines);
    let valid: Vec<Polygon<f64>> = faces
        .into_iter()
        .filter(|p| {
            let ext = &p.exterior().0;
            ext.len() >= 4
                && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                && crate::util::shoelace_abs_sum(ext) >= 1e-12
        })
        .collect();
    if valid.is_empty() {
        None
    } else {
        Some(MultiPolygon::new(valid))
    }
}

fn polygonize_mp(mp: &MultiPolygon<f64>) -> Option<MultiPolygon<f64>> {
    let lines: Vec<geo::Line<f64>> = mp.0.iter().flat_map(|p| p.lines_iter()).collect();
    if lines.is_empty() {
        return None;
    }
    let noded = crate::arrange::prep::prepare_lines(lines).ok()?;
    if noded.lines.is_empty() {
        return None;
    }
    let faces = crate::structure::polygonizer::polygonize(&noded.lines);
    let valid: Vec<Polygon<f64>> = faces
        .into_iter()
        .filter(|p| {
            let ext = &p.exterior().0;
            ext.len() >= 4
                && !ext.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
                && crate::util::shoelace_abs_sum(ext) >= 1e-12
        })
        .collect();
    if valid.is_empty() {
        None
    } else {
        Some(MultiPolygon::new(valid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Coord, LineString};

    fn make_shell() -> Polygon<f64> {
        Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        )
    }

    fn make_hole() -> Polygon<f64> {
        Polygon::new(
            LineString::new(vec![
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 8.0, y: 2.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 2.0, y: 8.0 },
                Coord { x: 2.0, y: 2.0 },
            ]),
            Vec::new(),
        )
    }

    #[test]
    fn test_subtract_holes_empty() {
        let shell = make_shell();
        let result = subtract_holes(&shell, &[]);
        assert!(!result.0.is_empty());
    }

    #[test]
    fn test_subtract_holes_single() {
        let shell = make_shell();
        let hole = make_hole();
        let result = subtract_holes(&shell, &[hole]);
        assert!(!result.0.is_empty());
        assert_eq!(result.0[0].interiors().len(), 1);
    }

    #[test]
    fn test_subtract_holes_multiple() {
        let shell = make_shell();
        let hole1 = make_hole();
        let hole2 = Polygon::new(
            LineString::new(vec![
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 8.0, y: 2.0 },
                Coord { x: 8.0, y: 8.0 },
                Coord { x: 2.0, y: 8.0 },
                Coord { x: 2.0, y: 2.0 },
            ]),
            Vec::new(),
        );
        let result = subtract_holes(&shell, &[hole1, hole2]);
        assert!(!result.0.is_empty());
    }

    #[test]
    fn test_subtract_holes_hole_equals_shell() {
        let shell = make_shell();
        let hole = make_shell();
        let result = subtract_holes(&shell, &[hole]);
        assert!(result.0.is_empty());
    }

    #[test]
    fn test_subtract_holes_hole_outside_shell() {
        let shell = make_shell();
        let hole = Polygon::new(
            LineString::new(vec![
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 25.0, y: 20.0 },
                Coord { x: 25.0, y: 25.0 },
                Coord { x: 20.0, y: 25.0 },
                Coord { x: 20.0, y: 20.0 },
            ]),
            Vec::new(),
        );
        let result = subtract_holes(&shell, &[hole]);
        assert!(!result.0.is_empty());
    }
}

use geo::{Coord, LineString};

/// Check if a hole ring is entirely outside a shell ring by testing its first point.
/// A simple non-self-intersecting ring is either entirely inside or entirely outside
/// a simple shell — partial overlap is impossible. So the first point is sufficient.
/// If the first point is on the shell boundary (rare), we classify as outer; the
/// boolean difference and merge steps handle this correctly either way.
fn is_hole_outside(hole: &LineString<f64>, shell: &LineString<f64>) -> bool {
    let first = match hole.0.first() {
        Some(pt) => *pt,
        None => return true,
    };
    // If the first point is strictly inside the shell, the hole is inner.
    if crate::simd::point_in_ring_exclusive(first, &shell.0) {
        return false;
    }
    // First point was outside or on the shell boundary.  Holes that share
    // boundary with the shell (rare) may have their first vertex exactly on
    // the shell.  Check all vertices as a robustness fallback.
    let mut any_outside = false;
    for pt in &hole.0 {
        if crate::simd::point_in_ring_exclusive(*pt, &shell.0) {
            return false;
        }
        // Strictly outside = not inside AND not on the boundary.
        if !crate::simd::point_in_ring_inclusive(*pt, &shell.0) {
            any_outside = true;
        }
    }
    // Boundary-touching hole: ALL vertices on the shell boundary (e.g. a
    // diamond hole touching the shell at 4 edge midpoints — CGAL
    // square_hole_rhombus). No vertex is strictly inside, but the hole
    // INTERIOR is inside the shell: it must be classified INNER and
    // subtracted (boolean difference splits the shell into 4 components),
    // NOT outer. Classifying it outer makes merge_shells' unary_union fold
    // it back into the shell — area 1.0 instead of GEOS's 0.5 (measured).
    // A simple ring's centroid is strictly inside iff the ring interior is
    // inside; for a truly outside ring the centroid is outside too.
    //
    // CRITICAL: only apply the centroid test when NO vertex is strictly
    // outside. hole==shell (all vertices on boundary, centroid inside) and
    // hole-larger-than-shell (some vertices outside) must keep the OUTER
    // classification — flipping them to inner loses the whole polygon
    // (measured: fuzz invariants 3.7/3.8 regressed to DuplicatedRings and
    // HoleOutsideShell).
    if !any_outside && !ring_set_equal(hole, shell) {
        let mut cx = 0.0;
        let mut cy = 0.0;
        let n = hole.0.len();
        if n > 0 {
            for pt in &hole.0 {
                cx += pt.x;
                cy += pt.y;
            }
            cx /= n as f64;
            cy /= n as f64;
            if crate::simd::point_in_ring_exclusive(Coord { x: cx, y: cy }, &shell.0) {
                return false;
            }
        }
    }
    true
}

/// True if two rings are the same coordinate set (rotation/reversal/
/// closure-insensitive). hole==shell is a degenerate invalid polygon whose
/// hole must NOT be reclassified inner by the centroid test — the
/// cancel_identical_shells path in merge_shells handles it (GEOS drops the
/// duplicate hole, yielding the shell; empty is also valid).
fn ring_set_equal(a: &LineString<f64>, b: &LineString<f64>) -> bool {
    if a.0.len() != b.0.len() {
        return false;
    }
    let mut aset: Vec<(u64, u64)> = a
        .0
        .iter()
        .map(|c| (c.x.to_bits(), c.y.to_bits()))
        .collect();
    let mut bset: Vec<(u64, u64)> = b
        .0
        .iter()
        .map(|c| (c.x.to_bits(), c.y.to_bits()))
        .collect();
    // Closure duplicates: last == first appears in both.
    if aset.first() == aset.last() {
        aset.pop();
    }
    if bset.first() == bset.last() {
        bset.pop();
    }
    if aset.len() != bset.len() {
        return false;
    }
    aset.sort_unstable();
    bset.sort_unstable();
    aset == bset
}

pub(crate) fn classify_holes(
    shell: &LineString<f64>,
    holes: &[LineString<f64>],
) -> (Vec<LineString<f64>>, Vec<LineString<f64>>) {
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        use rayon::prelude::*;
        let shell_bbox = bbox(shell);
        let classified: Vec<(LineString<f64>, bool)> = holes
            .par_iter()
            .map(|hole| {
                if !bboxes_overlap(shell_bbox, bbox(hole)) {
                    return (hole.clone(), true);
                }
                let is_outside = is_hole_outside(hole, shell);
                (hole.clone(), is_outside)
            })
            .collect();
        let mut inner = Vec::new();
        let mut outer = Vec::new();
        for (hole, is_outside) in classified {
            if is_outside {
                outer.push(hole);
            } else {
                inner.push(hole);
            }
        }
        (inner, outer)
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        let mut inner = Vec::new();
        let mut outer = Vec::new();
        let shell_bbox = bbox(shell);
        for hole in holes {
            if !bboxes_overlap(shell_bbox, bbox(hole)) {
                outer.push(hole.clone());
                continue;
            }
            let is_outside = is_hole_outside(hole, shell);
            if is_outside {
                outer.push(hole.clone());
            } else {
                inner.push(hole.clone());
            }
        }
        (inner, outer)
    }
}

type Bbox = (f64, f64, f64, f64);

fn bbox(ring: &LineString<f64>) -> Bbox {
    crate::simd::aabb_minmax_simd(&ring.0)
}

fn bboxes_overlap(a: Bbox, b: Bbox) -> bool {
    a.0 <= b.1 && b.0 <= a.1 && a.2 <= b.3 && b.2 <= a.3
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Coord;

    fn make_shell() -> LineString<f64> {
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ])
    }

    fn make_inner_hole() -> LineString<f64> {
        LineString::new(vec![
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 8.0, y: 2.0 },
            Coord { x: 8.0, y: 8.0 },
            Coord { x: 2.0, y: 8.0 },
            Coord { x: 2.0, y: 2.0 },
        ])
    }

    fn make_outer_hole() -> LineString<f64> {
        LineString::new(vec![
            Coord { x: 20.0, y: 20.0 },
            Coord { x: 25.0, y: 20.0 },
            Coord { x: 25.0, y: 25.0 },
            Coord { x: 20.0, y: 25.0 },
            Coord { x: 20.0, y: 20.0 },
        ])
    }

    #[test]
    fn test_classify_holes_empty() {
        let shell = make_shell();
        let (inner, outer) = classify_holes(&shell, &[]);
        assert!(inner.is_empty());
        assert!(outer.is_empty());
    }

    #[test]
    fn test_classify_holes_inner() {
        let shell = make_shell();
        let hole = make_inner_hole();
        let (inner, outer) = classify_holes(&shell, &[hole]);
        assert_eq!(inner.len(), 1);
        assert!(outer.is_empty());
    }

    #[test]
    fn test_classify_holes_outer() {
        let shell = make_shell();
        let hole = make_outer_hole();
        let (inner, outer) = classify_holes(&shell, &[hole]);
        assert!(inner.is_empty());
        assert_eq!(outer.len(), 1);
    }

    #[test]
    fn test_classify_holes_mixed() {
        let shell = make_shell();
        let (inner, outer) = classify_holes(&shell, &[make_inner_hole(), make_outer_hole()]);
        assert_eq!(inner.len(), 1);
        assert_eq!(outer.len(), 1);
    }

    #[test]
    fn test_classify_holes_hole_on_boundary() {
        let shell = make_shell();
        let on_boundary = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 5.0, y: 0.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 0.0, y: 5.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);
        let (inner, outer) = classify_holes(&shell, &[on_boundary]);
        // Point on boundary is not Outside → classified as inner
        assert_eq!(inner.len(), 1);
        assert!(outer.is_empty());
    }
}

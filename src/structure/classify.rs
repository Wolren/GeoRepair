use geo::LineString;

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
    for pt in &hole.0 {
        if crate::simd::point_in_ring_exclusive(*pt, &shell.0) {
            return false;
        }
    }
    true
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

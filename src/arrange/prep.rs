use rustc_hash::{FxHashMap, FxHashSet};

use crate::core::MakeValidError;
use crate::orient::orient2d;
use crate::snap;
use geo::{Coord, Line};
use rstar::{RTree, RTreeObject, AABB};

/// Prepared edge set: snapped, deduplicated, and intersection-split segments
/// ready for CDT construction.
#[derive(Debug)]
pub(crate) struct PreparedLines {
    /// The processed line segments (snapped, deduped, split at intersections).
    pub lines: Vec<Line<f64>>,
}

pub(crate) fn prepare_lines(lines: Vec<Line<f64>>) -> Result<PreparedLines, MakeValidError> {
    for line in &lines {
        for c in [line.start, line.end] {
            if c.x.is_nan() || c.y.is_nan() {
                return Err(MakeValidError::CoordinateIsNaN { idx: 0 });
            }
        }
    }
    // Use grid-hashed spatial lookup for O(n) snap dedup
    let snap_radius = 1e-8;
    let cell_size = snap_radius * 2.0;
    let mut grid: FxHashMap<(i64, i64), Coord<f64>> = FxHashMap::default();
    let mut snapped = Vec::with_capacity(lines.len());
    for mut line in lines {
        line.start = snap_or_push_grid(line.start, &mut grid, snap_radius, cell_size);
        line.end = snap_or_push_grid(line.end, &mut grid, snap_radius, cell_size);
        if line.start != line.end {
            snapped.push(line);
        }
    }
    odd_even_filter(&mut snapped);
    let mut split = split_segments(snapped)?;
    odd_even_filter(&mut split);
    Ok(PreparedLines { lines: split })
}

fn grid_key(coord: Coord<f64>, cell_size: f64) -> (i64, i64) {
    let x = coord.x / cell_size;
    let y = coord.y / cell_size;
    let xi = if x.is_finite() {
        x.floor().clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0i64
    };
    let yi = if y.is_finite() {
        y.floor().clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0i64
    };
    (xi, yi)
}

fn snap_or_push_grid(
    coord: Coord<f64>,
    grid: &mut FxHashMap<(i64, i64), Coord<f64>>,
    snap_radius: f64,
    cell_size: f64,
) -> Coord<f64> {
    let key = grid_key(coord, cell_size);
    // Check this cell and all 8 neighbors (use saturating arithmetic for edge cells)
    for dc in -1..=1i64 {
        for dr in -1..=1i64 {
            if let Some(&existing) = grid.get(&(key.0.saturating_add(dc), key.1.saturating_add(dr)))
            {
                let dx = coord.x - existing.x;
                let dy = coord.y - existing.y;
                if dx * dx + dy * dy <= snap_radius {
                    return existing;
                }
            }
        }
    }
    grid.insert(key, coord);
    coord
}

pub(crate) use super::prep_intersect::has_no_intersections;
pub(crate) fn odd_even_filter(lines: &mut Vec<Line<f64>>) {
    for line in lines.iter_mut() {
        if coord_greater(line.start, line.end) {
            std::mem::swap(&mut line.start, &mut line.end);
        }
    }
    lines.sort_by(|a, b| coord_cmp(a.start, b.start).then(coord_cmp(a.end, b.end)));
    let mut result = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let start = i;
        while i < lines.len() && lines[i] == lines[start] {
            i += 1;
        }
        if (i - start) % 2 == 1 {
            result.push(lines[start]);
        }
    }
    *lines = result;
}

fn coord_greater(a: Coord<f64>, b: Coord<f64>) -> bool {
    a.x > b.x || (a.x == b.x && a.y > b.y)
}

fn coord_cmp(a: Coord<f64>, b: Coord<f64>) -> std::cmp::Ordering {
    a.x.partial_cmp(&b.x)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
}

fn on_segment(a: Coord<f64>, b: Coord<f64>, c: Coord<f64>, eps: f64) -> bool {
    // Must be collinear
    if orient2d(a, b, c).abs() > eps {
        return false;
    }
    let minx = a.x.min(b.x) - eps;
    let maxx = a.x.max(b.x) + eps;
    let miny = a.y.min(b.y) - eps;
    let maxy = a.y.max(b.y) + eps;
    c.x >= minx && c.x <= maxx && c.y >= miny && c.y <= maxy
}

/// Returns the intersection point of two segments if they properly cross.
/// For collinear overlaps, returns the endpoints of the overlapping portion.
fn segment_intersection(
    a: Coord<f64>,
    b: Coord<f64>,
    c: Coord<f64>,
    d: Coord<f64>,
) -> Option<Vec<Coord<f64>>> {
    let eps = 1e-12;
    let o1 = orient2d(a, b, c);
    let o2 = orient2d(a, b, d);
    let o3 = orient2d(c, d, a);
    let o4 = orient2d(c, d, b);

    // Proper intersection (segments cross at interior points)
    if o1.abs() > eps && o2.abs() > eps && o3.abs() > eps && o4.abs() > eps {
        let s1 = o1.signum() != o2.signum();
        let s2 = o3.signum() != o4.signum();
        if s1 && s2 {
            let denom = (b.x - a.x) * (d.y - c.y) - (b.y - a.y) * (d.x - c.x);
            if denom.abs() < eps {
                return None;
            }
            let t = ((c.x - a.x) * (d.y - c.y) - (c.y - a.y) * (d.x - c.x)) / denom;
            let pt = Coord {
                x: a.x + t * (b.x - a.x),
                y: a.y + t * (b.y - a.y),
            };
            return Some(vec![pt]);
        }
        return None;
    }

    // Collinear or touching cases
    let collinear = o1.abs() < eps && o2.abs() < eps;
    if !collinear {
        // Endpoint touching — check if any endpoint lies on the other segment
        if o1.abs() < eps && on_segment(a, b, c, eps) {
            return Some(vec![c]);
        }
        if o2.abs() < eps && on_segment(a, b, d, eps) {
            return Some(vec![d]);
        }
        if o3.abs() < eps && on_segment(c, d, a, eps) {
            return Some(vec![a]);
        }
        if o4.abs() < eps && on_segment(c, d, b, eps) {
            return Some(vec![b]);
        }
        return None;
    }

    // Collinear overlap
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 < eps {
        return None;
    }
    let t_c = ((c.x - a.x) * dx + (c.y - a.y) * dy) / len2;
    let t_d = ((d.x - a.x) * dx + (d.y - a.y) * dy) / len2;
    let seg1_start = 0.0f64;
    let seg1_end = 1.0f64;
    let seg2_start = t_c.min(t_d);
    let seg2_end = t_c.max(t_d);
    let overlap_start = seg1_start.max(seg2_start);
    let overlap_end = seg1_end.min(seg2_end);
    if overlap_end - overlap_start <= eps {
        return None;
    }
    let p_start = Coord {
        x: a.x + overlap_start * dx,
        y: a.y + overlap_start * dy,
    };
    let p_end = Coord {
        x: a.x + overlap_end * dx,
        y: a.y + overlap_end * dy,
    };
    Some(vec![p_start, p_end])
}

fn split_segments(lines: Vec<Line<f64>>) -> Result<Vec<Line<f64>>, MakeValidError> {
    let n = lines.len();
    let mut split_points: Vec<Vec<(f64, Coord<f64>)>> = vec![Vec::new(); n];

    struct SegEnv {
        idx: usize,
        env: AABB<[f64; 2]>,
    }
    impl RTreeObject for SegEnv {
        type Envelope = AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }
    let envs: Vec<SegEnv> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| SegEnv {
            idx: i,
            env: AABB::from_corners(
                [l.start.x.min(l.end.x), l.start.y.min(l.end.y)],
                [l.start.x.max(l.end.x), l.start.y.max(l.end.y)],
            ),
        })
        .collect();
    let tree = RTree::bulk_load(envs);
    let mut seen = FxHashSet::default();
    use std::ops::ControlFlow;

    for i in 0..n {
        let li = &lines[i];
        let lo_x = li.start.x.min(li.end.x);
        let hi_x = li.start.x.max(li.end.x);
        let lo_y = li.start.y.min(li.end.y);
        let hi_y = li.start.y.max(li.end.y);
        let query = AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]);
        let _ = tree.locate_in_envelope_intersecting_int(&query, |c| {
            let j = c.idx;
            if j <= i {
                return ControlFlow::<(), ()>::Continue(());
            }
            let key = (i as u64) << 32 | (j as u64);
            if !seen.insert(key) {
                return ControlFlow::<(), ()>::Continue(());
            }
            let lj = &lines[j];
            if let Some(pts) = segment_intersection(li.start, li.end, lj.start, lj.end) {
                for pt in pts {
                    let is_end_i = (pt == li.start) || (pt == li.end);
                    if !is_end_i {
                        let ti = project_param(li, pt);
                        if ti > 0.0 && ti < 1.0 {
                            split_points[i].push((ti, pt));
                        }
                    }
                    let is_end_j = (pt == lj.start) || (pt == lj.end);
                    if !is_end_j {
                        let tj = project_param(lj, pt);
                        if tj > 0.0 && tj < 1.0 {
                            split_points[j].push((tj, pt));
                        }
                    }
                }
            }
            ControlFlow::<(), ()>::Continue(())
        });
    }

    let eps_param = 1e-14;
    let mut result = Vec::new();
    for i in 0..n {
        let line = lines[i];
        let mut splits: Vec<(f64, Coord<f64>)> = std::mem::take(&mut split_points[i]);
        splits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        splits.dedup_by(|a, b| (a.0 - b.0).abs() < eps_param);

        let mut prev_t = 0.0f64;
        let mut prev_pt = line.start;
        for &(t, pt) in &splits {
            if (t - prev_t).abs() < eps_param {
                continue;
            }
            let dx = pt.x - prev_pt.x;
            let dy = pt.y - prev_pt.y;
            if dx * dx + dy * dy > 1e-20 {
                result.push(Line::new(prev_pt, pt));
            }
            prev_t = t;
            prev_pt = pt;
        }
        let dx = line.end.x - prev_pt.x;
        let dy = line.end.y - prev_pt.y;
        if dx * dx + dy * dy > 1e-20 {
            result.push(Line::new(prev_pt, line.end));
        }
    }

    Ok(snap::snap_lines(result))
}

fn project_param(line: &Line<f64>, pt: Coord<f64>) -> f64 {
    let dx = line.end.x - line.start.x;
    let dy = line.end.y - line.start.y;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-30 {
        return 0.0;
    }
    ((pt.x - line.start.x) * dx + (pt.y - line.start.y) * dy) / len2
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------
    // odd_even_filter
    // -------------------------------

    #[test]
    fn test_odd_even_filter_empty() {
        let mut lines = Vec::new();
        odd_even_filter(&mut lines);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_odd_even_filter_single() {
        let mut lines = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        )];
        odd_even_filter(&mut lines);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_odd_even_filter_even_duplicates() {
        let mut lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
        ];
        odd_even_filter(&mut lines);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_odd_even_filter_odd_duplicates() {
        let l = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
        let mut lines = vec![l, l, l];
        odd_even_filter(&mut lines);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], l);
    }

    #[test]
    fn test_odd_even_filter_normalizes_orientation() {
        let l = Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 0.0, y: 0.0 });
        let mut lines = vec![l];
        odd_even_filter(&mut lines);
        // Start should be < end lexicographically
        assert!(
            lines[0].start.x < lines[0].end.x
                || (lines[0].start.x == lines[0].end.x && lines[0].start.y < lines[0].end.y)
        );
    }

    // -------------------------------
    // segment_intersection
    // -------------------------------

    #[test]
    fn test_segment_intersection_proper_crossing() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 2.0, y: 0.0 },
        );
        assert!(result.is_some());
        let pts = result.unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].x - 1.0).abs() < 1e-12);
        assert!((pts[0].y - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_segment_intersection_no_intersection() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 3.0, y: 3.0 },
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_intersection_endpoint_touching() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 1.0 },
        );
        assert!(result.is_some());
        let pts = result.unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].x - 1.0).abs() < 1e-12);
        assert_eq!(pts[0].y, 0.0);
    }

    #[test]
    fn test_segment_intersection_t_junction() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 2.0 },
        );
        assert!(result.is_some());
        let pts = result.unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].x - 1.0).abs() < 1e-12);
        assert_eq!(pts[0].y, 0.0);
    }

    #[test]
    fn test_segment_intersection_collinear_overlap() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 3.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 4.0, y: 0.0 },
        );
        assert!(result.is_some());
        let pts = result.unwrap();
        assert!(pts.len() >= 2);
    }

    #[test]
    fn test_segment_intersection_collinear_no_overlap() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 3.0, y: 0.0 },
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_intersection_parallel_non_intersecting() {
        let result = segment_intersection(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_segment_intersection_degenerate_zero_length() {
        let result = segment_intersection(
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 2.0 },
        );
        assert!(result.is_none());
    }

    // -------------------------------
    // on_segment
    // -------------------------------

    #[test]
    fn test_on_segment_midpoint() {
        assert!(on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            1e-12,
        ));
    }

    #[test]
    fn test_on_segment_endpoint() {
        assert!(on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            1e-12,
        ));
    }

    #[test]
    fn test_on_segment_not_on_segment() {
        assert!(!on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 3.0, y: 0.0 },
            1e-12,
        ));
    }

    #[test]
    fn test_on_segment_above() {
        assert!(!on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            1e-12,
        ));
    }

    #[test]
    fn test_on_segment_near_miss() {
        assert!(!on_segment(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 1e-11 },
            1e-12,
        ));
    }

    // -------------------------------
    // coord_greater / coord_cmp
    // -------------------------------

    #[test]
    fn test_coord_greater_x() {
        assert!(coord_greater(
            Coord { x: 2.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
        ));
        assert!(!coord_greater(
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 2.0, y: 0.0 },
        ));
    }

    #[test]
    fn test_coord_greater_y_tiebreaker() {
        assert!(coord_greater(
            Coord { x: 1.0, y: 2.0 },
            Coord { x: 1.0, y: 1.0 },
        ));
        assert!(!coord_greater(
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 2.0 },
        ));
    }

    #[test]
    fn test_coord_greater_equal() {
        assert!(!coord_greater(
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
        ));
    }

    // -------------------------------
    // project_param
    // -------------------------------

    #[test]
    fn test_project_param_midpoint() {
        let line = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 });
        let t = project_param(&line, Coord { x: 1.0, y: 1.0 });
        assert!((t - 0.5).abs() < 1e-12);
    }

    #[test]
    fn test_project_param_start() {
        let line = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 3.0, y: 4.0 });
        let t = project_param(&line, Coord { x: 1.0, y: 2.0 });
        assert!((t - 0.0).abs() < 1e-12);
    }

    #[test]
    fn test_project_param_end() {
        let line = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 3.0, y: 4.0 });
        let t = project_param(&line, Coord { x: 3.0, y: 4.0 });
        assert!((t - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_project_param_zero_length_line() {
        let line = Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 });
        let t = project_param(&line, Coord { x: 1.0, y: 1.0 });
        assert_eq!(t, 0.0);
    }

    #[test]
    fn test_project_param_off_line() {
        let line = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 0.0 });
        let t = project_param(&line, Coord { x: 1.0, y: 1.0 });
        assert!((t - 0.5).abs() < 1e-12);
    }

    // -------------------------------
    // snap_or_push_grid
    // -------------------------------

    #[test]
    fn test_snap_or_push_new() {
        let mut grid = FxHashMap::default();
        let snap_radius = 1e-8;
        let cell_size = snap_radius * 2.0;
        let result = snap_or_push_grid(Coord { x: 5.0, y: 5.0 }, &mut grid, snap_radius, cell_size);
        assert_eq!(result, Coord { x: 5.0, y: 5.0 });
        assert_eq!(grid.len(), 1);
    }

    #[test]
    fn test_snap_or_push_existing() {
        let mut grid = FxHashMap::default();
        let snap_radius = 1e-8;
        let cell_size = snap_radius * 2.0;
        grid.insert(
            grid_key(Coord { x: 0.0, y: 0.0 }, cell_size),
            Coord { x: 0.0, y: 0.0 },
        );
        let result = snap_or_push_grid(
            Coord { x: 1e-10, y: 1e-10 },
            &mut grid,
            snap_radius,
            cell_size,
        );
        assert_eq!(result, Coord { x: 0.0, y: 0.0 });
        assert_eq!(grid.len(), 1);
    }

    #[test]
    fn test_snap_or_push_far() {
        let mut grid = FxHashMap::default();
        let snap_radius = 1e-8;
        let cell_size = snap_radius * 2.0;
        let result = snap_or_push_grid(
            Coord { x: 0.01, y: 0.01 },
            &mut grid,
            snap_radius,
            cell_size,
        );
        assert_eq!(result, Coord { x: 0.01, y: 0.01 });
        assert_eq!(grid.len(), 1);
    }

    // -------------------------------
    // prepare_lines integration
    // -------------------------------

    #[test]
    fn test_prepare_lines_empty() {
        let result = prepare_lines(Vec::new());
        assert!(result.is_ok());
        let prepared = result.unwrap();
        assert!(prepared.lines.is_empty());
    }

    #[test]
    fn test_prepare_lines_single() {
        let lines = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        )];
        let result = prepare_lines(lines);
        assert!(result.is_ok());
        let prepared = result.unwrap();
        assert_eq!(prepared.lines.len(), 1);
    }

    #[test]
    fn test_prepare_lines_with_nan() {
        let lines = vec![Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: f64::NAN,
                y: 0.0,
            },
        )];
        let result = prepare_lines(lines);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MakeValidError::CoordinateIsNaN { .. }
        ));
    }

    #[test]
    fn test_split_segments_no_intersections() {
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 2.0, y: 0.0 }, Coord { x: 3.0, y: 0.0 }),
        ];
        let result = split_segments(lines.clone());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn test_split_segments_crossing() {
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }),
            Line::new(Coord { x: 0.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }),
        ];
        let result = split_segments(lines);
        assert!(result.is_ok());
        let split = result.unwrap();
        // Should split each crossing segment into 2, plus snap
        assert!(split.len() >= 3);
    }

    #[test]
    fn diagnose_many_holes() {
        use geo::LinesIter;
        use std::time::Instant;

        let mut wkt = String::from("POLYGON ((0 0, 100 0, 100 100, 0 100, 0 0)");
        for i in 0..10 {
            let x = 2.0 + (i as f64) * 8.0;
            let y = 2.0;
            wkt.push_str(&format!(
                ", ({x} {y}, {} {y}, {} {}, {x} {}, {x} {y})",
                x + 3.0,
                x + 3.0,
                y + 3.0,
                y + 3.0
            ));
        }
        wkt.push(')');

        let geo: geo::Geometry<f64> = wkt::TryFromWkt::try_from_wkt_str(&wkt).unwrap();
        let poly = match geo {
            geo::Geometry::Polygon(p) => p,
            _ => panic!("nope"),
        };

        let lines: Vec<_> = poly.lines_iter().collect();

        // warmup
        for _ in 0..100 {
            let _ = has_no_intersections(&lines);
        }

        let start = Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(has_no_intersections(&lines));
        }
        let d = start.elapsed();
        println!("has_no_intersections x1000: {:?}, per: {:?}", d, d / 1000);

        let start = Instant::now();
        for _ in 0..1000 {
            let _ = std::hint::black_box(poly.clone());
        }
        let d = start.elapsed();
        println!("poly.clone x1000: {:?}, per: {:?}", d, d / 1000);

        let start = Instant::now();
        for _ in 0..1000 {
            let _ = std::hint::black_box(crate::arrange::poly_has_basic_form(&poly));
        }
        let d = start.elapsed();
        println!("poly_has_basic_form x1000: {:?}, per: {:?}", d, d / 1000);

        // Does has_no_intersections return true?
        let hni = has_no_intersections(&lines);
        println!("has_no_intersections returned: {hni}");

        let config = crate::core::MakeValidConfig {
            poly_method: crate::core::PolyMethod::Arrange,
            ..Default::default()
        };

        for _ in 0..1000 {
            std::hint::black_box(crate::arrange::fix_polygon(&poly, &config));
        }
    }
}

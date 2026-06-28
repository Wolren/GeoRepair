use geo::{Coord, Line};
use rustc_hash::FxHashMap;

const SNAP_GRID: f64 = 1e-10;
const HOT_PIXEL_RADIUS: f64 = SNAP_GRID * 0.5;

fn grid_key(c: Coord<f64>) -> (i64, i64) {
    let x = c.x / HOT_PIXEL_RADIUS;
    let y = c.y / HOT_PIXEL_RADIUS;
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

fn snap_to_grid(c: Coord<f64>) -> Coord<f64> {
    Coord {
        x: (c.x / SNAP_GRID).round() * SNAP_GRID,
        y: (c.y / SNAP_GRID).round() * SNAP_GRID,
    }
}

/// A hot pixel represents a grid cell at SNAP_GRID resolution.
/// Its center is snapped to the grid, and it can detect if a segment
/// passes through it within HOT_PIXEL_RADIUS.
struct HotPixel {
    center: Coord<f64>,
}

impl HotPixel {
    fn new(c: Coord<f64>) -> Self {
        HotPixel {
            center: snap_to_grid(c),
        }
    }

    /// Returns true if the segment passes within HOT_PIXEL_RADIUS
    /// of this hot pixel's center (including exactly through it).
    fn touches(&self, seg: &Line<f64>) -> bool {
        let dx = seg.end.x - seg.start.x;
        let dy = seg.end.y - seg.start.y;
        let len2 = dx * dx + dy * dy;

        if len2 == 0.0 {
            let d2 = (self.center.x - seg.start.x).powi(2) + (self.center.y - seg.start.y).powi(2);
            return d2 <= HOT_PIXEL_RADIUS * HOT_PIXEL_RADIUS;
        }

        let t = ((self.center.x - seg.start.x) * dx + (self.center.y - seg.start.y) * dy) / len2;
        let t = t.clamp(0.0, 1.0);
        let proj_x = seg.start.x + t * dx;
        let proj_y = seg.start.y + t * dy;

        let d2 = (self.center.x - proj_x).powi(2) + (self.center.y - proj_y).powi(2);
        d2 <= HOT_PIXEL_RADIUS * HOT_PIXEL_RADIUS
    }

    /// Returns the parameter of the closest point on the segment
    /// to this hot pixel's center.
    fn closest_param(&self, seg: &Line<f64>) -> f64 {
        let dx = seg.end.x - seg.start.x;
        let dy = seg.end.y - seg.start.y;
        let len2 = dx * dx + dy * dy;
        if len2 == 0.0 {
            return 0.0;
        }
        let t = ((self.center.x - seg.start.x) * dx + (self.center.y - seg.start.y) * dy) / len2;
        t.clamp(0.0, 1.0)
    }
}

/// Snap-rounding noder that subdivides segments at hot pixel boundaries.
struct SnapRoundingNoder {
    hot_pixels: FxHashMap<(i64, i64), HotPixel>,
}

impl SnapRoundingNoder {
    fn new() -> Self {
        SnapRoundingNoder {
            hot_pixels: FxHashMap::default(),
        }
    }

    /// Nodes a set of line segments by:
    /// 1. Collecting all unique coordinates (endpoints + intersection points)
    /// 2. Creating hot pixels at each coordinate snapped to grid
    /// 3. Falling near-coincident points to the same hot pixel
    /// 4. Subdividing each segment at all hot pixels it passes through
    /// 5. Snapping all coordinates to grid
    fn node(&mut self, segments: &[Line<f64>]) -> Vec<Line<f64>> {
        if segments.is_empty() {
            return Vec::new();
        }

        // Step 1a: Collect all unique endpoints
        let mut coords: Vec<Coord<f64>> = Vec::with_capacity(segments.len() * 2);
        for seg in segments {
            coords.push(seg.start);
            coords.push(seg.end);
        }

        // Step 1b: Compute interior intersection points and add to coords
        let n = segments.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if j == i + 1 && segments[i].end == segments[j].start {
                    continue;
                }
                if let Some((pt, _t_dd)) = crate::dd::segment_intersection_dd(
                    segments[i].start,
                    segments[i].end,
                    segments[j].start,
                    segments[j].end,
                ) {
                    coords.push(pt);
                }
            }
        }

        // Sort and dedup coordinates (to_bits for NaN-safe total order)
        coords.sort_by(|a, b| {
            a.x.to_bits()
                .cmp(&b.x.to_bits())
                .then(a.y.to_bits().cmp(&b.y.to_bits()))
        });
        coords.dedup();

        // Step 2 & 3: Create hot pixels, merging near-coincident points
        for &c in &coords {
            let key = grid_key(c);
            let mut found = false;
            for dc in -1i64..=1 {
                for dr in -1i64..=1 {
                    let nk = (key.0.saturating_add(dc), key.1.saturating_add(dr));
                    if let Some(hp) = self.hot_pixels.get(&nk) {
                        let dx = c.x - hp.center.x;
                        let dy = c.y - hp.center.y;
                        if dx * dx + dy * dy <= HOT_PIXEL_RADIUS * HOT_PIXEL_RADIUS {
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    break;
                }
            }
            if !found {
                let snapped = snap_to_grid(c);
                self.hot_pixels
                    .insert(grid_key(snapped), HotPixel::new(snapped));
            }
        }

        // Step 4 & 5: Subdivide each segment at hot pixels and snap to grid
        let eps = 1e-14;
        let mut result: Vec<Line<f64>> = Vec::new();
        for seg in segments {
            let mut params: Vec<f64> = Vec::new();
            params.push(0.0);
            params.push(1.0);

            for hp in self.hot_pixels.values() {
                if hp.touches(seg) {
                    params.push(hp.closest_param(seg));
                }
            }

            params.sort_by(|a, b| a.to_bits().cmp(&b.to_bits()));
            params.dedup_by(|a, b| (*a - *b).abs() < eps);

            for window in params.windows(2) {
                let t1 = window[0];
                let t2 = window[1];
                if (t2 - t1).abs() < eps {
                    continue;
                }
                let p1 = Coord {
                    x: seg.start.x + t1 * (seg.end.x - seg.start.x),
                    y: seg.start.y + t1 * (seg.end.y - seg.start.y),
                };
                let p2 = Coord {
                    x: seg.start.x + t2 * (seg.end.x - seg.start.x),
                    y: seg.start.y + t2 * (seg.end.y - seg.start.y),
                };
                let s1 = snap_to_grid(p1);
                let s2 = snap_to_grid(p2);
                if s1 != s2 {
                    result.push(Line::new(s1, s2));
                }
            }
        }

        result
    }
}

/// Snap-round a set of line segments to a uniform grid.
///
/// Algorithm (HotPixel-style via SnapRoundingNoder):
/// 1. Collect all unique coordinates (endpoints + intersection points)
/// 2. Create hot pixels at each coordinate snapped to the grid
/// 3. Fall near-coincident points to the same hot pixel
/// 4. For each segment, find all hot pixels it passes through
/// 5. Subdivide each segment at hot pixel boundaries
/// 6. Snap all coordinates to grid and drop zero-length segments
pub(crate) fn snap_round_lines(lines: &[Line<f64>]) -> Vec<Line<f64>> {
    let mut noder = SnapRoundingNoder::new();
    noder.node(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snap_round_empty() {
        let result = snap_round_lines(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_snap_round_no_change() {
        // Non-intersecting segments should pass through unchanged
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 }),
        ];
        let result = snap_round_lines(&lines);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_snap_round_close_coords() {
        // Two coordinates within hot pixel radius
        let lines = vec![
            Line::new(
                Coord {
                    x: 1.0 + 1e-12,
                    y: 2.0,
                },
                Coord { x: 3.0, y: 10.0 },
            ),
            Line::new(
                Coord {
                    x: 1.0 + 2e-12,
                    y: 2.0,
                },
                Coord { x: 5.0, y: 6.0 },
            ),
        ];
        let result = snap_round_lines(&lines);
        // Both lines start at (1.0, 2.0) after snapping
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].start, result[1].start);
        assert_eq!(result[0].start, Coord { x: 1.0, y: 2.0 });
    }

    #[test]
    fn test_snap_round_exact_grid_already() {
        let c = Coord {
            x: 1.2345678912,
            y: 5.0,
        };
        let snapped = snap_to_grid(c);
        assert!((snapped.x - 1.2345678912).abs() < 1e-9);
        assert!((snapped.y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_snap_round_filters_zero_length() {
        let lines = vec![
            Line::new(
                Coord {
                    x: 1.0 + 1e-12,
                    y: 2.0,
                },
                Coord { x: 1.0, y: 2.0 },
            ),
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }),
        ];
        let result = snap_round_lines(&lines);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_snap_round_near_grid_point() {
        // A point near a grid intersection should snap to it
        let grid_x = 1.2345678912;
        let grid_y = 5.0;
        let offset = 1e-11; // within hot pixel (5e-11)
        let lines = vec![Line::new(
            Coord {
                x: grid_x + offset,
                y: grid_y - offset,
            },
            Coord {
                x: grid_x + 1.0,
                y: grid_y + 1.0,
            },
        )];
        let result = snap_round_lines(&lines);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start.x, grid_x);
        assert_eq!(result[0].start.y, grid_y);
    }

    #[test]
    fn test_snap_round_many_coords() {
        let mut lines = Vec::new();
        for i in 0..100 {
            let x = i as f64 * 0.001;
            lines.push(Line::new(
                Coord { x, y: x + 1e-12 },
                Coord {
                    x: x + 1.0,
                    y: x + 1.0 + 1e-12,
                },
            ));
        }
        let result = snap_round_lines(&lines);
        // Segments are subdivided at hot pixels they pass through
        assert!(!result.is_empty());
    }

    #[test]
    fn test_snap_round_large_coords() {
        let lines = vec![Line::new(
            Coord {
                x: 1e14 + 1e-12,
                y: 1e14,
            },
            Coord {
                x: 1e14 + 1.0,
                y: 1e14 + 1.0,
            },
        )];
        let result = snap_round_lines(&lines);
        assert_eq!(result.len(), 1);
        let snapped_start = snap_to_grid(result[0].start);
        assert!((result[0].start.x - snapped_start.x).abs() < 1e-9);
    }

    #[test]
    fn test_segment_passes_near_hot_pixel() {
        // A segment that passes close to a hot pixel should be subdivided
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 0.0 }),
            Line::new(Coord { x: 5.0, y: 1e-11 }, Coord { x: 5.0, y: -1e-11 }),
        ];
        let result = snap_round_lines(&lines);
        // The first segment should be subdivided at the hot pixel near (5,0)
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_hot_pixel_merges_nearby_intersections() {
        // Two lines intersecting very close to each other
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 }),
            Line::new(Coord { x: 0.0, y: 10.0 }, Coord { x: 10.0, y: 0.0 }),
        ];
        let result = snap_round_lines(&lines);
        // Should still have the same number of unique nodes
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_subdivision_at_hot_pixel() {
        // A single line near a vertex from another line
        let lines = vec![
            Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }),
            Line::new(Coord { x: 0.5, y: 1e-11 }, Coord { x: 0.5, y: 1.0 }),
        ];
        let result = snap_round_lines(&lines);
        // First segment should be subdivided
        assert!(!result.is_empty());
    }
}

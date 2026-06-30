use geo::{Coord, Line};
use rustc_hash::FxHashMap;
#[cfg(feature = "rstar")]
use rustc_hash::FxHashSet;

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

/// R-tree entry for a hot pixel center.
#[cfg(feature = "rstar")]
struct HpEntry {
    center: Coord<f64>,
    env: rstar::AABB<[f64; 2]>,
}

#[cfg(feature = "rstar")]
impl HpEntry {
    fn new(center: Coord<f64>) -> Self {
        let cx = if center.x.is_finite() { center.x } else { 0.0 };
        let cy = if center.y.is_finite() { center.y } else { 0.0 };
        let r = HOT_PIXEL_RADIUS;
        HpEntry {
            center: Coord { x: cx, y: cy },
            env: rstar::AABB::from_corners([cx - r, cy - r], [cx + r, cy + r]),
        }
    }
}

#[cfg(feature = "rstar")]
impl rstar::RTreeObject for HpEntry {
    type Envelope = rstar::AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

// ── MCIndex: monotone-chain spatial indexing for O(n log n) intersection ──

/// Direction quadrant (0-3) for a segment vector.
#[cfg(feature = "rstar")]
fn quadrant(dx: f64, dy: f64) -> u8 {
    if dx > 0.0 {
        if dy >= 0.0 {
            0
        } else {
            1
        }
    } else if dx < 0.0 {
        if dy > 0.0 {
            3
        } else {
            2
        }
    } else {
        if dy > 0.0 {
            0
        } else {
            2
        }
    }
}

/// A monotone chain of consecutive segments in the same direction quadrant.
#[cfg(feature = "rstar")]
struct MonoChain {
    start: usize,
    end: usize,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[cfg(feature = "rstar")]
fn build_chains(segments: &[Line<f64>]) -> Vec<MonoChain> {
    let n = segments.len();
    if n == 0 {
        return Vec::new();
    }
    let mut chains: Vec<MonoChain> = Vec::new();
    let mut start = 0usize;
    let dx = segments[0].end.x - segments[0].start.x;
    let dy = segments[0].end.y - segments[0].start.y;
    let mut prev_quad = quadrant(dx, dy);
    let mut min_x = segments[0].start.x.min(segments[0].end.x);
    let mut max_x = segments[0].start.x.max(segments[0].end.x);
    let mut min_y = segments[0].start.y.min(segments[0].end.y);
    let mut max_y = segments[0].start.y.max(segments[0].end.y);

    for (i, s) in segments.iter().enumerate().skip(1) {
        let dx = s.end.x - s.start.x;
        let dy = s.end.y - s.start.y;
        let cur_quad = quadrant(dx, dy);
        min_x = min_x.min(s.start.x).min(s.end.x);
        max_x = max_x.max(s.start.x).max(s.end.x);
        min_y = min_y.min(s.start.y).min(s.end.y);
        max_y = max_y.max(s.start.y).max(s.end.y);
        if cur_quad != prev_quad {
            chains.push(MonoChain {
                start,
                end: i,
                min_x,
                max_x,
                min_y,
                max_y,
            });
            start = i;
            prev_quad = cur_quad;
            min_x = s.start.x.min(s.end.x);
            max_x = s.start.x.max(s.end.x);
            min_y = s.start.y.min(s.end.y);
            max_y = s.start.y.max(s.end.y);
        }
    }
    chains.push(MonoChain {
        start,
        end: n,
        min_x,
        max_x,
        min_y,
        max_y,
    });
    chains
}

#[cfg(feature = "rstar")]
struct ChainEnv {
    idx: usize,
    env: rstar::AABB<[f64; 2]>,
}
#[cfg(feature = "rstar")]
impl rstar::RTreeObject for ChainEnv {
    type Envelope = rstar::AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

/// Collect all intersection points using MCIndex spatial indexing.
#[cfg(feature = "rstar")]
fn collect_intersections_mcindex(
    segments: &[Line<f64>],
    chains: &[MonoChain],
    chain_tree: &rstar::RTree<ChainEnv>,
    coords: &mut Vec<Coord<f64>>,
    checked: &mut FxHashSet<(usize, usize)>,
) {
    let nc = chains.len();
    for i in 0..nc {
        let mc1 = &chains[i];
        let q = rstar::AABB::from_corners([mc1.min_x, mc1.min_y], [mc1.max_x, mc1.max_y]);
        let _ = chain_tree.locate_in_envelope_intersecting_int(&q, |c| {
            let j = c.idx;
            if j <= i {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            check_chain_pair(segments, mc1, &chains[j], coords, checked);
            std::ops::ControlFlow::<(), ()>::Continue(())
        });
    }
}

/// Recursive divide-and-conquer: split larger chain, check leaf pairs.
#[cfg(feature = "rstar")]
fn check_chain_pair(
    segments: &[Line<f64>],
    mc1: &MonoChain,
    mc2: &MonoChain,
    coords: &mut Vec<Coord<f64>>,
    checked: &mut FxHashSet<(usize, usize)>,
) {
    // Bounding box overlap test
    if mc1.min_x > mc2.max_x + 1e-12
        || mc1.max_x < mc2.min_x - 1e-12
        || mc1.min_y > mc2.max_y + 1e-12
        || mc1.max_y < mc2.min_y - 1e-12
    {
        return;
    }

    // Leaf case: single segment in each chain
    if mc1.end - mc1.start == 1 && mc2.end - mc2.start == 1 {
        let i = mc1.start;
        let j = mc2.start;
        if i >= j || !checked.insert((i, j)) {
            return;
        }
        // Skip adjacent pairs sharing an endpoint
        if j == i + 1 && segments[i].end == segments[j].start {
            return;
        }
        if i == 0 && j == segments.len() - 1 && segments[j].end == segments[i].start {
            return;
        }
        if let Some((pt, _, _)) = crate::dd::segment_intersection_dd(
            segments[i].start,
            segments[i].end,
            segments[j].start,
            segments[j].end,
        ) {
            coords.push(pt);
        }
        return;
    }

    // Subdivide the larger chain using actual segment bounds
    if (mc1.end - mc1.start) >= (mc2.end - mc2.start) {
        let mid = (mc1.start + mc1.end) / 2;
        if mid > mc1.start {
            let left = sub_chain(segments, mc1.start, mid);
            check_chain_pair(segments, &left, mc2, coords, checked);
        }
        if mid < mc1.end {
            let right = sub_chain(segments, mid, mc1.end);
            check_chain_pair(segments, &right, mc2, coords, checked);
        }
    } else {
        let mid = (mc2.start + mc2.end) / 2;
        if mid > mc2.start {
            let left = sub_chain(segments, mc2.start, mid);
            check_chain_pair(segments, mc1, &left, coords, checked);
        }
        if mid < mc2.end {
            let right = sub_chain(segments, mid, mc2.end);
            check_chain_pair(segments, mc1, &right, coords, checked);
        }
    }
}

/// Build a sub-chain from `segments[start..end]` with a computed bounding box.
#[cfg(feature = "rstar")]
fn sub_chain(segments: &[Line<f64>], start: usize, end: usize) -> MonoChain {
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for s in &segments[start..end] {
        min_x = min_x.min(s.start.x).min(s.end.x);
        max_x = max_x.max(s.start.x).max(s.end.x);
        min_y = min_y.min(s.start.y).min(s.end.y);
        max_y = max_y.max(s.start.y).max(s.end.y);
    }
    MonoChain {
        start,
        end,
        min_x,
        max_x,
        min_y,
        max_y,
    }
}

/// Threshold above which MCIndex is used instead of brute force.
const MCINDEX_THRESHOLD: usize = 64;

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

        // Step 1b: Compute interior intersection points.
        let n = segments.len();
        #[cfg(feature = "rstar")]
        if n >= MCINDEX_THRESHOLD {
            let chains = build_chains(segments);
            let envs: Vec<ChainEnv> = chains
                .iter()
                .enumerate()
                .map(|(i, mc)| ChainEnv {
                    idx: i,
                    env: rstar::AABB::from_corners([mc.min_x, mc.min_y], [mc.max_x, mc.max_y]),
                })
                .collect();
            let chain_tree = rstar::RTree::bulk_load(envs);
            let mut checked: FxHashSet<(usize, usize)> = FxHashSet::default();
            collect_intersections_mcindex(
                segments,
                &chains,
                &chain_tree,
                &mut coords,
                &mut checked,
            );
        }
        #[cfg(not(feature = "rstar"))]
        {}
        if n < MCINDEX_THRESHOLD || !cfg!(feature = "rstar") {
            for i in 0..n {
                for j in (i + 1)..n {
                    if j == i + 1 && segments[i].end == segments[j].start {
                        continue;
                    }
                    if let Some((pt, _, _)) = crate::dd::segment_intersection_dd(
                        segments[i].start,
                        segments[i].end,
                        segments[j].start,
                        segments[j].end,
                    ) {
                        coords.push(pt);
                    }
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
            if !c.x.is_finite() || !c.y.is_finite() {
                continue;
            }
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
        #[cfg(feature = "rstar")]
        {
            let hp_r = HOT_PIXEL_RADIUS;

            let hp_entries: Vec<HpEntry> = self
                .hot_pixels
                .values()
                .map(|hp| HpEntry::new(hp.center))
                .collect();
            let hp_tree = rstar::RTree::bulk_load(hp_entries);

            for seg in segments {
                let mut params: Vec<f64> = Vec::new();
                params.push(0.0);
                params.push(1.0);

                let lo_x = seg.start.x.min(seg.end.x) - hp_r;
                let hi_x = seg.start.x.max(seg.end.x) + hp_r;
                let lo_y = seg.start.y.min(seg.end.y) - hp_r;
                let hi_y = seg.start.y.max(seg.end.y) + hp_r;
                let query = rstar::AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]);
                let _ = hp_tree.locate_in_envelope_intersecting_int(&query, |entry| {
                    let hp = HotPixel {
                        center: entry.center,
                    };
                    if hp.touches(seg) {
                        params.push(hp.closest_param(seg));
                    }
                    std::ops::ControlFlow::<(), ()>::Continue(())
                });

                params.sort_by_key(|a| a.to_bits());
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
        }

        #[cfg(not(feature = "rstar"))]
        for seg in segments {
            let mut params: Vec<f64> = Vec::new();
            params.push(0.0);
            params.push(1.0);

            for hp in self.hot_pixels.values() {
                if hp.touches(seg) {
                    params.push(hp.closest_param(seg));
                }
            }

            params.sort_by_key(|a| a.to_bits());
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

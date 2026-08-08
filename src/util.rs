//! Shared utility functions used across the crate.

use alloc::vec::Vec;
use geo::Coord;

pub(crate) fn shoelace_sum(ring: &[Coord<f64>]) -> f64 {
    let mut sum = 0.0;
    for window in ring.windows(2) {
        sum += window[0].x * window[1].y - window[1].x * window[0].y;
    }
    sum
}

/// Determine CCW winding of a ring using Shewchuk's orient2d on the
/// rightmost-lowest vertex (guaranteed convex for simple rings).
/// Handles extreme fp ratios where shoelace sum flips sign.
pub(crate) fn robust_is_ccw(ring: &[Coord<f64>]) -> bool {
    if ring.len() < 4 {
        return false;
    }
    robust_is_ccw_at(ring, min_x_vertex(ring))
}

/// CCW verdict plus the extremal vertex index (min x, tie min y) in one
/// pass - the caller can re-verify a possibly-reversed ring with
/// [`robust_is_ccw_at`] without re-searching (winding fusion, 2026-08-08).
pub(crate) fn robust_is_ccw_with_index(ring: &[Coord<f64>]) -> (bool, usize) {
    let idx = min_x_vertex(ring);
    (robust_is_ccw_at(ring, idx), idx)
}

/// The extremal vertex index used by [`robust_is_ccw`]: minimum x, tie
/// minimum y, candidate at index 0 (the closure point for closed rings).
pub(crate) fn min_x_vertex(ring: &[Coord<f64>]) -> usize {
    if ring.len() < 4 {
        return 0;
    }
    let interior_n = ring.len() - 1;
    let mut min_idx = 0;
    let mut min_x = ring[0].x;
    let mut min_y = ring[0].y;
    for (k, c) in ring[1..interior_n].iter().enumerate() {
        let i = k + 1;
        if c.x < min_x || (c.x == min_x && c.y < min_y) {
            min_x = c.x;
            min_y = c.y;
            min_idx = i;
        }
    }
    min_idx
}

/// Shewchuk orient2d at a pre-located extremal vertex (see
/// [`min_x_vertex`]): the search is skipped, the verdict is identical.
/// Collinear extremal vertex → shoelace fallback (Shewchuk gives 0).
pub(crate) fn robust_is_ccw_at(ring: &[Coord<f64>], min_idx: usize) -> bool {
    if ring.len() < 4 {
        return false;
    }
    let interior_n = ring.len() - 1;
    let prev = if min_idx == 0 {
        &ring[interior_n - 1]
    } else {
        &ring[min_idx - 1]
    };
    let curr = &ring[min_idx];
    let next = &ring[(min_idx + 1) % interior_n];
    let orient = robust::orient2d(
        robust::Coord {
            x: prev.x,
            y: prev.y,
        },
        robust::Coord {
            x: curr.x,
            y: curr.y,
        },
        robust::Coord {
            x: next.x,
            y: next.y,
        },
    );
    if orient.abs() <= 1e-15 {
        shoelace_sum(ring) > 0.0
    } else {
        orient > 0.0
    }
}

/// A point strictly inside the ring: midpoint of the first non-degenerate
/// edge, nudged toward the ring interior by a relative epsilon (1e-9 of the
/// local magnitude). The interior side follows the ring's signed area
/// (CCW: left of the directed edge; CW: right). Returns None only for
/// degenerate rings (fewer than 3 distinct edges or all-zero-length edges).
///
/// Used for hole-aware nesting tests: vertex probes lie ON a hole ring for
/// islands that touch it, and exclusive point-in-ring semantics then read
/// the vertex as "not in the hole" (misclassifying island-in-hole as
/// nested-in-fill). A strictly-interior probe resolves the ambiguity.
pub(crate) fn ring_interior_probe(ring: &[Coord<f64>]) -> Option<Coord<f64>> {
    let interior_n = ring.len().saturating_sub(1);
    if interior_n < 3 {
        return None;
    }
    let mut sa = 0.0;
    for i in 0..interior_n {
        let a = ring[i];
        let b = ring[(i + 1) % interior_n];
        sa += a.x * b.y - b.x * a.y;
    }
    let sign = if sa >= 0.0 { 1.0 } else { -1.0 };
    for i in 0..interior_n {
        let v0 = ring[i];
        let v1 = ring[(i + 1) % interior_n];
        let dx = v1.x - v0.x;
        let dy = v1.y - v0.y;
        let len2 = dx * dx + dy * dy;
        if len2 <= 1e-24 {
            continue;
        }
        let len = len2.sqrt();
        let edge_mid = Coord {
            x: (v0.x + v1.x) * 0.5,
            y: (v0.y + v1.y) * 0.5,
        };
        let scale = edge_mid.x.abs().max(edge_mid.y.abs()).max(1.0);
        let eps = 1e-9 * scale;
        let (nx, ny) = (-dy / len, dx / len);
        return Some(Coord {
            x: edge_mid.x + nx * eps * sign,
            y: edge_mid.y + ny * eps * sign,
        });
    }
    None
}

/// structure/subtract.rs.
pub(crate) fn shoelace_abs_sum(coords: &[Coord<f64>]) -> f64 {
    let n = coords.len();
    if n < 3 {
        return 0.0;
    }
    let end = if coords.first() == coords.last() {
        n - 1
    } else {
        n
    };
    let mut sum = 0.0_f64;
    for i in 0..end - 1 {
        sum += coords[i].x * coords[i + 1].y - coords[i + 1].x * coords[i].y;
    }
    sum += coords[end - 1].x * coords[0].y - coords[0].x * coords[end - 1].y;
    sum.abs()
}

/// Rotation/order-insensitive coordinate fingerprint of a ring (exact bit
/// patterns, sorted). Was duplicated in structure/build_area.rs and
/// structure/merge.rs.
pub(crate) fn ring_fingerprint(ring: &[Coord<f64>]) -> Vec<(u64, u64)> {
    let mut pts: Vec<(u64, u64)> = ring
        .iter()
        .map(|c| (c.x.to_bits(), c.y.to_bits()))
        .collect();
    if pts.first() == pts.last() {
        pts.pop();
    }
    pts.sort_unstable();
    pts
}

/// Length of the intersection of two 1-D intervals (0 if disjoint).
/// Was duplicated in structure/subtract.rs and arrange/mod.rs.
#[inline]
pub(crate) fn interval_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    if hi > lo { hi - lo } else { 0.0 }
}

/// True if two rings share a collinear edge (both directions, exact after
/// noding). Bbox-prefiltered O(na*nb) scan, capped to small rings.
///
/// TWO CALIBRATIONS exist for this test (formerly one private copy per
/// caller — the merge into this module is what makes the split explicit):
///
/// - `rings_share_collinear_edge_quantized`: absolute tolerance with a 1.0
///   magnitude floor (cross/line tests at 1e-12 * scale^2, overlap at
///   1e-12 * scale). For rings that came out of i_overlay's boolean
///   difference, whose coordinates are quantized to a fixed 1e-9 grid:
///   a hole edge that was exactly parallel to the shell edge gains a
///   ~1e-10 angular deviation, and the tight relative test misses the
///   shared-edge signal (measured: valid 4-hole square at scale 48 →
///   DisconnectedInteriorRing when the relative test is used here).
/// - `rings_share_collinear_edge_precise`: relative tolerance
///   (|cross| / (|da| * |db|) = sin(angle)). For PRE-quantization rings
///   (fuzz generators, raw input) the absolute floor misclassifies
///   genuinely non-collinear tiny edges as collinear (measured: two
///   1.6e-7 edges with cross 2.8e-14 < 1e-12 → false DIR).
pub(crate) fn rings_share_collinear_edge_quantized(a: &[Coord<f64>], b: &[Coord<f64>]) -> bool {
    rings_bbox_overlap_prefilter(a, b)
        && rings_scan(
            a,
            b,
            |ax1,
             ay1,
             _ax2,
             _ay2,
             dax,
             day,
             dbx,
             dby,
             aminx,
             amaxx,
             aminy,
             amaxy,
             bx1,
             bx2,
             by1,
             by2| {
                let scale = dax
                    .abs()
                    .max(day.abs())
                    .max(dbx.abs())
                    .max(dby.abs())
                    .max(1.0);
                let rel = 1e-12 * scale * scale;
                let cross = dax * dby - day * dbx;
                if cross.abs() > rel {
                    return None;
                }
                let cross2 = (bx2 - bx1) * (ay1 - by1) - (by2 - by1) * (ax1 - bx1);
                if cross2.abs() > rel {
                    return None;
                }
                let overlap = if dax.abs() >= day.abs() {
                    interval_overlap(aminx, amaxx, bx1.min(bx2), bx1.max(bx2))
                } else {
                    interval_overlap(aminy, amaxy, by1.min(by2), by1.max(by2))
                };
                (overlap > 1e-12 * scale).then_some(())
            },
        )
}

pub(crate) fn rings_share_collinear_edge_precise(a: &[Coord<f64>], b: &[Coord<f64>]) -> bool {
    // NO bbox prefilter here: the relative collinearity eps tolerates 1-ULP
    // offsets at touching coordinates (adjacent holes sharing an edge whose
    // x differs by 1 ULP — measured: 16.338582962195097 vs ...098), and an
    // EXACT bbox test rejects such pairs outright (the shared edge would
    // never be scanned → DisconnectedInteriorRing). The quantized variant
    // keeps its prefilter because post-noding coordinates are exact.
    rings_scan(
        a,
        b,
        |ax1,
         ay1,
         _ax2,
         _ay2,
         dax,
         day,
         dbx,
         dby,
         aminx,
         amaxx,
         aminy,
         amaxy,
         bx1,
         bx2,
         by1,
         by2| {
            // Relative collinearity: |cross| / (|da| * |db|) = sin(angle).
            let da_len = (dax * dax + day * day).sqrt().max(1e-300);
            let db_len = (dbx * dbx + dby * dby).sqrt().max(1e-300);
            let rel = 1e-12 * da_len * db_len;
            let cross = dax * dby - day * dbx;
            if cross.abs() > rel {
                return None;
            }
            let cross2 = (bx2 - bx1) * (ay1 - by1) - (by2 - by1) * (ax1 - bx1);
            if cross2.abs() > rel {
                return None;
            }
            // Positive-length overlap only: a shared endpoint is length 0.
            let overlap = if dax.abs() >= day.abs() {
                interval_overlap(aminx, amaxx, bx1.min(bx2), bx1.max(bx2))
            } else {
                interval_overlap(aminy, amaxy, by1.min(by2), by1.max(by2))
            };
            (overlap > 1e-12 * da_len.max(db_len)).then_some(())
        },
    )
}

fn rings_bbox_overlap_prefilter(a: &[Coord<f64>], b: &[Coord<f64>]) -> bool {
    let bbox = |r: &[Coord<f64>]| {
        let mut lo = (f64::INFINITY, f64::INFINITY);
        let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for c in r {
            lo = (lo.0.min(c.x), lo.1.min(c.y));
            hi = (hi.0.max(c.x), hi.1.max(c.y));
        }
        (lo, hi)
    };
    let (alo, ahi) = bbox(a);
    let (blo, bhi) = bbox(b);
    alo.0 <= bhi.0 && ahi.0 >= blo.0 && alo.1 <= bhi.1 && ahi.1 >= blo.1
}

fn rings_scan(
    a: &[Coord<f64>],
    b: &[Coord<f64>],
    mut pair: impl FnMut(
        f64,
        f64,
        f64,
        f64, // ax1, ay1, ax2, ay2
        f64,
        f64,
        f64,
        f64, // dax, day, dbx, dby
        f64,
        f64,
        f64,
        f64, // aminx, amaxx, aminy, amaxy
        f64,
        f64,
        f64,
        f64, // bx1, bx2, by1, by2
    ) -> Option<()>,
) -> bool {
    let na = if a.first() == a.last() {
        a.len() - 1
    } else {
        a.len()
    };
    let nb = if b.first() == b.last() {
        b.len() - 1
    } else {
        b.len()
    };
    if na < 2 || nb < 2 {
        return false;
    }
    // Work cap: only small rings get the precise scan (fuzz DIR seeds are tiny).
    if na * nb > 2048 {
        return false;
    }
    for i in 0..na {
        let (ax1, ay1) = (a[i].x, a[i].y);
        let (ax2, ay2) = (a[(i + 1) % na].x, a[(i + 1) % na].y);
        for j in 0..nb {
            let (bx1, by1) = (b[j].x, b[j].y);
            let (bx2, by2) = (b[(j + 1) % nb].x, b[(j + 1) % nb].y);
            let dax = ax2 - ax1;
            let day = ay2 - ay1;
            let dbx = bx2 - bx1;
            let dby = by2 - by1;
            let aminx = ax1.min(ax2);
            let amaxx = ax1.max(ax2);
            let aminy = ay1.min(ay2);
            let amaxy = ay1.max(ay2);
            if pair(
                ax1, ay1, ax2, ay2, dax, day, dbx, dby, aminx, amaxx, aminy, amaxy, bx1, bx2, by1,
                by2,
            )
            .is_some()
            {
                return true;
            }
        }
    }
    false
}

/// Even-odd toggle point-in-ring test (exclusive: on-edge → outside).
/// One of two point-in-ring families in the crate: this one uses the
/// even-odd ray-cast toggle and rejects rings with fewer than 4 coords;
/// `validation::point_in_ring_exclusive` uses the winding number and
/// accepts rings with 2+ coords. For simple rings both agree. NOT unified
/// yet: the degenerate-ring guards differ and each call site relies on its
/// own guard semantics (ring_simplicity at every call site must be
/// verified before merging the families).
/// Was duplicated in structure/merge.rs and make_valid.rs.
pub(crate) fn point_in_ring_exclusive_even_odd(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    if ring.len() < 4 {
        return false;
    }
    let n = ring.len() - 1;
    // Boundary check (exclusive: on-edge → outside). Fixes NestedHoles
    // false-positives from ray-cast hitting a vertex/edge.
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[(i + 1) % n].x, ring[(i + 1) % n].y);
        let orient = (xi - pt.x) * (yj - pt.y) - (xj - pt.x) * (yi - pt.y);
        if orient.abs() < 1e-15 {
            let min_x = xi.min(xj);
            let max_x = xi.max(xj);
            let min_y = yi.min(yj);
            let max_y = yi.max(yj);
            if pt.x >= min_x - 1e-12
                && pt.x <= max_x + 1e-12
                && pt.y >= min_y - 1e-12
                && pt.y <= max_y + 1e-12
            {
                return false;
            }
        }
    }
    let mut inside = false;
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[(i + 1) % n].x, ring[(i + 1) % n].y);
        let intersect =
            ((yi > pt.y) != (yj > pt.y)) && (pt.x < (xj - xi) * (pt.y - yi) / (yj - yi) + xi);
        if intersect {
            inside = !inside;
        }
    }
    inside
}

/// Wall clock for the `PROFILE_*` counters and DIAG_* diagnostics.
///
/// `std::time::Instant` is not implemented on wasm32-unknown-unknown -
/// every call panics with "time not implemented on this platform". The
/// clock degrades to a no-op on wasm (0 ns) so the profiling surface stays
/// valid API there while the repair path keeps running. (Found 2026-08-06
/// by the first wasm runtime test: the repair test panicked in
/// fix_polygon_owned's PROFILE_FP_NS timing.)
#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
pub(crate) struct ProfileClock(std::time::Instant);

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
impl ProfileClock {
    pub(crate) fn start() -> Self {
        Self(std::time::Instant::now())
    }
    pub(crate) fn ns(&self) -> u64 {
        self.0.elapsed().as_nanos() as u64
    }
}

#[cfg(any(target_arch = "wasm32", not(feature = "std")))]
pub(crate) struct ProfileClock;

#[cfg(any(target_arch = "wasm32", not(feature = "std")))]
impl ProfileClock {
    pub(crate) fn start() -> Self {
        Self
    }
    pub(crate) fn ns(&self) -> u64 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shoelace_sum_ccw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let sum = shoelace_sum(&ring);
        assert!(sum > 0.0);
        assert!((sum - 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_shoelace_sum_cw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let sum = shoelace_sum(&ring);
        assert!(sum < 0.0);
        assert!((sum + 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_robust_is_ccw_ccw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(robust_is_ccw(&ring));
    }

    #[test]
    fn test_robust_is_ccw_cw() {
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        assert!(!robust_is_ccw(&ring));
    }

    #[test]
    fn test_robust_is_ccw_extreme_fp() {
        // Ring with coords spanning 1e12 to 1e-12 — shoelace can flip
        let ring = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1e12, y: 0.0 },
            Coord { x: 1e12, y: 1e-12 },
            Coord { x: 0.0, y: 1e-12 },
            Coord { x: 0.0, y: 0.0 },
        ];
        // Should be CCW regardless of fp extremes
        assert!(robust_is_ccw(&ring));
    }
}

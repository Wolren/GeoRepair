//! ring validity: closure, self-intersection, orientation, points, duplicates
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance:
//! core.rs was 2540 lines; the cap is 800). Content is verbatim - no
//! behavior changes; sibling modules resolve shared items through the
//! `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.



use alloc::vec::Vec;
use crate::validation::core::*;
use geo::{Coord};

pub(crate) fn ring_has_non_finite(ring: &[Coord<f64>]) -> bool {
    ring.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
}

pub fn check_ring_validity(
    ring: &[Coord<f64>],
    is_exterior: bool,
) -> Vec<GeometryValidationError> {
    let mut errors = Vec::new();

    if ring_has_non_finite(ring) {
        errors.push(GeometryValidationError::CoordinateNaN);
        return errors;
    }

    if ring.len() < 4 {
        errors.push(GeometryValidationError::RingTooFewPoints {
            found: ring.len(),
            min: 4,
        });
        return errors;
    }

    if ring.first() != ring.last() {
        errors.push(GeometryValidationError::RingNotClosed {
            first: ring[0],
            last: ring[ring.len() - 1],
        });
        return errors;
    }

    let n = ring.len() - 1;

    #[cfg(feature = "simd")]
    let (min_x, max_x, min_y, max_y) = crate::simd::aabb_minmax_simd(&ring[..n]);
    #[cfg(not(feature = "simd"))]
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    #[cfg(not(feature = "simd"))]
    for c in &ring[..n] {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * scale;
    // Per-axis degeneracy: an axis is collapsed only if its extent is below
    // EPSILON × that AXIS'S OWN max magnitude. Comparing against the
    // cross-axis scale wrongly flags thin-but-real triangles (e.g. base
    // 8.26e7, height 8.4e-9 - representable, GEOS-valid; measured: seed
    // 00c11200 sibling → DegenerateExterior on a triangle GEOS validates).
    let x_scale = max_x.abs().max(min_x.abs()).max(1.0);
    let y_scale = max_y.abs().max(min_y.abs()).max(1.0);
    if (max_x - min_x).abs() < f64::EPSILON * x_scale
        || (max_y - min_y).abs() < f64::EPSILON * y_scale
    {
        if is_exterior {
            errors.push(GeometryValidationError::DegenerateExterior);
        } else {
            errors.push(GeometryValidationError::CollinearRing);
        }
        return errors;
    }

    {
        let mut prev_coord = &ring[0];
        for c in &ring[1..n] {
            if c.x == prev_coord.x && c.y == prev_coord.y {
                errors.push(GeometryValidationError::RepeatedPoint);
                break;
            }
            prev_coord = c;
        }
    }

    let mut seen: rustc_hash::FxHashMap<(u64, u64), usize> =
        rustc_hash::FxHashMap::with_capacity_and_hasher(n, Default::default());
    for (idx, c) in ring[..n].iter().enumerate() {
        // Normalize -0.0 to +0.0 before keying: the IEEE bit patterns differ
        // but the coordinates are equal, and a pinch at the origin must be
        // detected regardless of zero sign (measured: differential fuzz,
        // repaired polygon with (0,0) and (-0,0) vertices was accepted).
        let key = ((c.x + 0.0).to_bits(), (c.y + 0.0).to_bits());
        if let Some(&prev) = seen.get(&key) {
            if prev + 1 == idx {
                continue;
            }
            errors.push(GeometryValidationError::PinchPoint);
            break;
        } else {
            seen.insert(key, idx);
        }
    }

    if n > 64 {
        // Sweep fast path: edges sorted by padded min_x (radix sort),
        // small x-overlap active set, padded-2D-bbox candidate gate, then
        // the exact pair predicate. Same semantics as the tree path:
        // adjacent ring-order pairs skipped, closing pair (0, n-1) tested,
        // each pair tested once. Rings with pathologically dense x-overlap
        // route to the spatial index (rstar) or brute force (non-rstar) -
        // exact predicates in all paths, a routing decision, not a
        // tolerance. Measured (2026-08-06): per-ring rstar bulk_load +
        // queries cost ~320ms on the 600k-vertex giants vs ~50ms for the
        // sweep; the biggest giant's active set averages 23 (max 63).
        match sweep_ring_self_intersects(ring, eps) {
            Some(true) => {
                errors.push(GeometryValidationError::SelfIntersection);
                return errors;
            }
            Some(false) => {}
            None => {
                #[cfg(feature = "rstar")]
                {
                    if ring_tree_self_intersects(ring, n, eps) {
                        errors.push(GeometryValidationError::SelfIntersection);
                        return errors;
                    }
                }
                #[cfg(not(feature = "rstar"))]
                {
                    for i in 0..n {
                        for j in i + 2..n {
                            if i == 0 && j == n - 1 {
                                continue;
                            }
                            if check_edge_pair_intersection(ring, i, j, eps) {
                                errors.push(GeometryValidationError::SelfIntersection);
                                return errors;
                            }
                        }
                    }
                }
            }
        }
    } else {
        // Small rings: brute force with a padded-bbox prefilter (the
        // exact predicates internally require bbox overlap, so rejecting
        // non-overlapping pairs here never changes results; the tree path
        // applied the same filter via envelope intersection).
        #[cfg(feature = "rstar")]
        {
            for i in 0..n {
                let a1 = ring[i];
                let a2 = ring[(i + 1) % n];
                for j in i + 2..n {
                    // Closing-edge pair (0, n-1) is NOT skipped: the edges
                    // share vertex 0 but may overlap collinearly beyond it
                    // (backtracking closure) - a genuine self-intersection.
                    // edges_intersect_general excludes endpoint-only touches.
                    // See the rstar branch comment (differential fuzz 2026-08-03).
                    if padded_bbox_overlap(a1, a2, ring[j], ring[(j + 1) % n])
                        && check_edge_pair_intersection(ring, i, j, eps)
                    {
                        errors.push(GeometryValidationError::SelfIntersection);
                        return errors;
                    }
                }
            }
        }
        #[cfg(not(feature = "rstar"))]
        {
            for i in 0..n {
                let a1 = ring[i];
                let a2 = ring[(i + 1) % n];
                for j in i + 2..n {
                    if i == 0 && j == n - 1 {
                        continue;
                    }
                    if padded_bbox_overlap(a1, a2, ring[j], ring[(j + 1) % n])
                        && check_edge_pair_intersection(ring, i, j, eps)
                    {
                        errors.push(GeometryValidationError::SelfIntersection);
                        return errors;
                    }
                }
            }
        }
    }

    errors
}

pub(crate) fn check_orientation(ring: &[Coord<f64>]) -> bool {
    if ring.len() < 4 {
        return true;
    }
    // Use Shewchuk's orient2d (adaptive precision) on the extremal vertex.
    // The shoelace sum can flip sign at extreme fp ratios (e.g. 1e12 and 1e-12).
    crate::util::robust_is_ccw(ring)
}

pub(crate) fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    let n = ring.len();
    if n < 2 {
        return false;
    }
    // Boundary check (exclusive: on-edge → outside)
    for i in 0..n - 1 {
        let p1 = ring[i];
        let p2 = ring[i + 1];
        let o = (p2.x - p1.x) * (pt.y - p1.y) - (p2.y - p1.y) * (pt.x - p1.x);
        if o.abs() < 1e-15 {
            let min_x = p1.x.min(p2.x);
            let max_x = p1.x.max(p2.x);
            let min_y = p1.y.min(p2.y);
            let max_y = p1.y.max(p2.y);
            if pt.x >= min_x - 1e-12 && pt.x <= max_x + 1e-12
                && pt.y >= min_y - 1e-12 && pt.y <= max_y + 1e-12
            {
                return false;
            }
        }
    }
    let mut wn = 0i32;
    for i in 0..n - 1 {
        let p1 = ring[i];
        let p2 = ring[i + 1];
        if p1.y <= pt.y {
            if p2.y > pt.y {
                let o = (p2.x - p1.x) * (pt.y - p1.y) - (p2.y - p1.y) * (pt.x - p1.x);
                if o > 0.0 {
                    wn += 1;
                }
            }
        } else if p2.y <= pt.y {
            let o = (p2.x - p1.x) * (pt.y - p1.y) - (p2.y - p1.y) * (pt.x - p1.x);
            if o < 0.0 {
                wn -= 1;
            }
        }
    }
    wn != 0
}

pub(crate) fn point_on_segment(pt: Coord<f64>, a: Coord<f64>, b: Coord<f64>, eps: f64) -> bool {
    let o = (b.x - a.x) * (pt.y - a.y) - (b.y - a.y) * (pt.x - a.x);
    if o.abs() > eps {
        return false;
    }
    let min_x = a.x.min(b.x) - eps;
    let max_x = a.x.max(b.x) + eps;
    let min_y = a.y.min(b.y) - eps;
    let max_y = a.y.max(b.y) + eps;
    pt.x >= min_x && pt.x <= max_x && pt.y >= min_y && pt.y <= max_y
}

/// EXACT point-on-segment (GEOS parity). Uses the robust orient2d so that
/// exactly-collinear representable points count (the naive cross product can
/// round to 1e-16 for them), but near-miss vertices do NOT touch: the
/// tolerance-based point_on_segment fabricated touches from near-miss
/// vertices in real cadastral giants (GEOS isValid=true on all 12 sampled,
/// measured 2026-08-06). GEOS's own predicates are exact, so the touch
/// graph must be exact too.
pub(crate) fn point_on_segment_exact(pt: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> bool {
    if crate::orient::orient2d(a, b, pt) != 0.0 {
        return false;
    }
    let (lo_x, hi_x) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
    let (lo_y, hi_y) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
    pt.x >= lo_x && pt.x <= hi_x && pt.y >= lo_y && pt.y <= hi_y
}

pub(crate) fn point_on_ring(pt: Coord<f64>, ring: &[Coord<f64>], eps: f64) -> bool {
    let n = ring.len() - 1;
    if n == 0 {
        return false;
    }
    for i in 0..n {
        if point_on_segment(pt, ring[i], ring[(i + 1) % n], eps) {
            return true;
        }
    }
    false
}

/// Fast rotation-invariant fingerprint for duplicate ring detection.
///
/// Finds the index of the lexicographically-minimum coordinate, hashes the
/// ring starting from that index in both forward and reverse directions, and
/// XORs the two hashes together.  Two rings that are rotated duplicates will
/// produce the same fingerprint regardless of winding order.
pub(crate) fn ring_dup_fingerprint(ring: &[Coord<f64>]) -> (usize, u64) {
    if ring.len() <= 1 {
        return (ring.len(), 0);
    }
    let n = ring.len() - 1;
    let min_idx = {
        let mut idx = 0usize;
        for i in 1..n {
            let c = ring[i];
            let m = ring[idx];
            if c.x < m.x || (c.x == m.x && c.y < m.y) {
                idx = i;
            }
        }
        idx
    };
    let mut h_fwd = 0u64;
    let mut h_rev = 0u64;
    for i in 0..n {
        let c = ring[(min_idx + i) % n];
        h_fwd = h_fwd
            .wrapping_mul(6364136223846793005)
            .wrapping_add(c.x.to_bits());
        h_fwd = h_fwd
            .wrapping_mul(6364136223846793005)
            .wrapping_add(c.y.to_bits());
        let d = ring[(min_idx + n - i) % n];
        h_rev = h_rev
            .wrapping_mul(6364136223846793005)
            .wrapping_add(d.x.to_bits());
        h_rev = h_rev
            .wrapping_mul(6364136223846793005)
            .wrapping_add(d.y.to_bits());
    }
    (ring.len(), h_fwd ^ h_rev)
}

/// Check whether two rings (with closing point) are duplicates starting at a
/// different vertex. Both rings must have the same length and contain the same
/// sequence of coordinates up to a cyclic rotation.
pub(crate) fn is_rotated_duplicate(a: &[Coord<f64>], b: &[Coord<f64>]) -> bool {
    if a.len() != b.len() || a.len() < 2 {
        return false;
    }
    // Rings: last == first, so compare n-1 vertices
    let n = a.len() - 1;
    if n == 0 {
        return false;
    }
    // Forward scan (same winding order)
    for start in 0..n {
        if a[start] != b[0] {
            continue;
        }
        let mut match_ = true;
        for i in 0..n {
            if a[(start + i) % n] != b[i] {
                match_ = false;
                break;
            }
        }
        if match_ {
            return true;
        }
    }
    // Reverse scan (opposite winding order)
    for start in 0..n {
        if a[start] != b[0] {
            continue;
        }
        let mut match_ = true;
        for i in 0..n {
            if a[(start + n - i) % n] != b[i] {
                match_ = false;
                break;
            }
        }
        if match_ {
            return true;
        }
    }
    false
}

/// [min_x, min_y, max_x, max_y]; degenerate rings get the zero box.
pub(crate) fn ring_bbox(ring: &[Coord<f64>]) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for c in ring {
        b[0] = b[0].min(c.x);
        b[1] = b[1].min(c.y);
        b[2] = b[2].max(c.x);
        b[3] = b[3].max(c.y);
    }
    if b[0] > b[2] {
        return [0.0, 0.0, 0.0, 0.0];
    }
    b
}

pub(crate) fn bbox_overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Does box `a` fully cover box `b`? (GEOS envelope-covers gate.)
pub(crate) fn bbox_covers(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[0] && a[2] >= b[2] && a[1] <= b[1] && a[3] >= b[3]
}

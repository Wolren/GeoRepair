//! hole validation, nesting, and cycle detection
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance:
//! core.rs was 2540 lines; the cap is 800). Content is verbatim - no
//! behavior changes; sibling modules resolve shared items through the
//! `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.



use alloc::vec::Vec;
use crate::validation::core::*;
use geo::{Coord, LineString};
#[cfg(feature = "rstar")]
use alloc::sync::Arc;

/// Per-hole boundary check (crossing / exact touches / any-inside), shared
/// by the serial and parallel per-hole phases. Returns the FIRST error in
/// the check order, or None. `tree` is the shell's edge tree (None for
/// small shells - the brute paths are faster there).
#[cfg(feature = "rstar")]
fn per_hole_check(
    shell: &[Coord<f64>],
    tree: Option<&rstar::RTree<EdgeIdx>>,
    hole: &LineString<f64>,
    eps: f64,
    max_x: f64,
) -> Option<GeometryValidationError> {
    // Check if hole edges cross the shell boundary (hole not fully inside)
    let crossing = match tree {
        Some(t) => ring_edges_intersect_tree(&hole.0[..], shell, t, eps),
        None => check_rings_intersect(&hole.0[..], shell, eps),
    };
    if crossing {
        return Some(GeometryValidationError::HoleOutsideShell);
    }

    // A hole touching the shell at >= 2 distinct points may disconnect
    // the interior. Note: the same vertex can be on 2+ edges of the
    // shell (outgoing + incoming), so we must deduplicate touch points.
    // Touch test is EXACT (point_on_segment_exact): tolerance-based
    // touches fabricated near-miss contacts on real cadastral giants
    // (GEOS isValid=true; measured 2026-08-06).
    let hole_touches: Vec<Coord<f64>> = match tree {
        Some(t) => ring_touch_points(&hole.0[..], shell, t),
        None => hole
            .0
            .iter()
            .copied()
            .filter(|&hp| point_on_ring(hp, shell, 0.0))
            .collect(),
    };
    let mut touch_count = 0usize;
    let mut seen_touches: Vec<Coord<f64>> = Vec::new();
    for hp in hole_touches {
        if !seen_touches.contains(&hp) {
            touch_count += 1;
            seen_touches.push(hp);
        }
    }
    if touch_count >= 2 {
        return Some(GeometryValidationError::DisconnectedInteriorRing);
    }

    // If no hole vertex is strictly inside the shell, the hole is
    // entirely outside. Single-point tangent touches (touch_count == 1)
    // are valid per OGC.
    let any_inside = match tree {
        Some(t) => hole
            .0
            .iter()
            .any(|&hp| point_in_ring_exclusive_tree(hp, shell, t, max_x)),
        None => hole.0.iter().any(|&hp| point_in_ring_exclusive(hp, shell)),
    };
    if !any_inside {
        return Some(GeometryValidationError::HoleOutsideShell);
    }
    None
}

/// Per-hole boundary check for non-rstar builds (identical semantics).
#[cfg(not(feature = "rstar"))]
fn per_hole_check(
    shell: &[Coord<f64>],
    hole: &LineString<f64>,
    eps: f64,
) -> Option<GeometryValidationError> {
    if check_rings_intersect(&hole.0[..], shell, eps) {
        return Some(GeometryValidationError::HoleOutsideShell);
    }
    let hole_touches: Vec<Coord<f64>> = hole
        .0
        .iter()
        .copied()
        .filter(|&hp| point_on_ring(hp, shell, 0.0))
        .collect();
    let mut touch_count = 0usize;
    let mut seen_touches: Vec<Coord<f64>> = Vec::new();
    for hp in hole_touches {
        if !seen_touches.contains(&hp) {
            touch_count += 1;
            seen_touches.push(hp);
        }
    }
    if touch_count >= 2 {
        return Some(GeometryValidationError::DisconnectedInteriorRing);
    }
    if !hole.0.iter().any(|&hp| point_in_ring_exclusive(hp, shell)) {
        return Some(GeometryValidationError::HoleOutsideShell);
    }
    None
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn check_holes_valid(
    shell: &[Coord<f64>],
    interiors: &[LineString<f64>],
) -> Vec<GeometryValidationError> {
    let mut errors = Vec::new();

    // Hole-less polygons (the vast majority of real-world data) skip all
    // boundary work: the aabb/eps and shell tree below exist only for the
    // per-hole checks. Measured: 1.58M hole-less polys paid a SIMD aabb
    // + eps for nothing (2026-08-06).
    if interiors.is_empty() {
        return errors;
    }

    // Compute scale-relative epsilon for boundary checks
    #[cfg(feature = "simd")]
    let (min_x, max_x, min_y, max_y) = crate::simd::aabb_minmax_simd(shell);
    #[cfg(not(feature = "simd"))]
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    #[cfg(not(feature = "simd"))]
    for c in shell {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * scale;

    // One shell edge tree per polygon (never per hole): the naive per-hole
    // calls paid O(|shell| x log|hole|) in check_rings_intersect plus
    // O(|hole| x |shell|) in point_on_ring - measured 18s on the 1.58M
    // real-world dataset (giants with ~100 holes x 187k-edge shells), vs
    // 0.8-2.3s for the cheap structural gate the README's validation row
    // documented. Tree queries per hole are O(|hole| log|shell|).
    #[cfg(feature = "rstar")]
    let shell_tree: Option<Arc<rstar::RTree<EdgeIdx>>> = (shell.len() - 1 > 64)
        .then(|| Arc::new(build_ring_edge_tree(shell)));

    // Giant shells: the per-hole checks are independent, but nested
    // parallelism (rayon::join + per-hole par_iter inside the batch) was
    // MEASURED as a regression (3.83s vs 3.17s, 2026-08-06) - the batch's
    // pool is already saturated, so nested work only adds split/join
    // overhead. The batch-level size partition is the parallel lever that
    // works; the per-hole phase stays serial.
    for hole in interiors {
        #[cfg(feature = "rstar")]
        let e = per_hole_check(shell, shell_tree.as_deref(), hole, eps, max_x);
        #[cfg(not(feature = "rstar"))]
        let e = per_hole_check(shell, hole, eps);
        if let Some(e) = e {
            errors.push(e);
            return errors;
        }
    }
    let holes: Vec<&[Coord<f64>]> = interiors.iter().map(|h| &h.0[..]).collect();
    if holes.len() > 1 {
        // --- hole-hole edge intersection check (disconnected interior) ---
        // Candidate pairs are bounding-box filtered (R-tree when there are
        // many rings; the pair list is shared with the touch-cycle and
        // nesting checks below) - never an unfiltered O(h^2) pair loop.
        let mut rings: Vec<&[Coord<f64>]> = Vec::with_capacity(holes.len() + 1);
        rings.push(shell);
        rings.extend(holes.iter().copied());
        let bboxes: Vec<[f64; 4]> = rings.iter().map(|r| ring_bbox(r)).collect();
        for (a, b) in overlap_pairs(&bboxes, 1) {
            if check_rings_intersect(rings[a], rings[b], eps) {
                errors.push(GeometryValidationError::DisconnectedInteriorRing);
                return errors;
            }
        }

        // --- hole-cycle detection (GEOS PolygonRing::findHoleCycleLocation
        // + scanForHoleCycle port, source-verified 2026-08-05): a cycle in
        // the ring-touch graph at pairwise-DISTINCT coordinates disconnects
        // the interior. Rings touching at a single coordinate (three holes
        // meeting at one point) stay valid (GEOS isValid=true) - the
        // same-coordinate skip inside detect_hole_cycle implements that.
        #[cfg(feature = "rstar")]
        let cycle = detect_hole_cycle(&rings, &bboxes, eps, shell_tree.as_ref());
        #[cfg(not(feature = "rstar"))]
        let cycle = detect_hole_cycle(&rings, &bboxes, eps);
        if cycle {
            errors.push(GeometryValidationError::DisconnectedInteriorRing);
            return errors;
        }

        // --- nesting check (GEOS IndexedNestedHoleTester +
        // PolygonTopologyAnalyzer::isRingNested port) ---
        // Candidate pairs: bbox-overlap (R-tree when many rings) PLUS the
        // GEOS envelope-covers gate. The probe handles a start point on the
        // target's boundary via the incident-segment topology - the old
        // point_in_ring_exclusive probe missed inner holes whose vertices
        // ALL lie on the outer hole's boundary (measured t20, 2026-08-05).
        for (a, b) in overlap_pairs(&bboxes, 1) {
            if !bbox_covers(bboxes[a], bboxes[b]) {
                continue;
            }
            if is_ring_nested(rings[b], rings[a], eps) {
                errors.push(GeometryValidationError::NestedHoles);
                return errors;
            }
        }
    }
    errors
}

// ---------------------------------------------------------------------------
// Ring-touch graph helpers (GEOS PolygonRing / PolygonTopologyAnalyzer /
// PolygonNodeTopology ports, source-verified against the GEOS clone at
// 24ec89dc3, 2026-08-05). All pair enumeration is bounding-box filtered:
// O(n log n) tree + candidate queries, never unfiltered O(n^2).
// ---------------------------------------------------------------------------

/// Candidate ring pairs (i < j) with overlapping bounding boxes, skipping
/// rings below `min_ring` (0 includes the shell, 1 = holes only). Small
/// ring counts brute-force (faster than building a tree); larger counts use
/// an R-tree under the rstar feature, with the all-pairs fallback otherwise.
pub(crate) fn overlap_pairs(bboxes: &[[f64; 4]], min_ring: usize) -> Vec<(usize, usize)> {
    let n = bboxes.len();
    if n <= 16 {
        let mut out = Vec::new();
        for i in min_ring..n {
            for j in (i + 1)..n {
                if bbox_overlap(bboxes[i], bboxes[j]) {
                    out.push((i, j));
                }
            }
        }
        return out;
    }
    #[cfg(feature = "rstar")]
    {
        struct RingEnv {
            idx: usize,
            env: rstar::AABB<[f64; 2]>,
        }
        impl rstar::RTreeObject for RingEnv {
            type Envelope = rstar::AABB<[f64; 2]>;
            fn envelope(&self) -> Self::Envelope {
                self.env
            }
        }
        let tree = rstar::RTree::bulk_load(
            (0..n)
                .map(|i| RingEnv {
                    idx: i,
                    env: rstar::AABB::from_corners(
                        [bboxes[i][0], bboxes[i][1]],
                        [bboxes[i][2], bboxes[i][3]],
                    ),
                })
                .collect::<Vec<_>>(),
        );
        let mut out = Vec::new();
        for (i, bbox) in bboxes.iter().enumerate().skip(min_ring) {
            let q = rstar::AABB::from_corners([bbox[0], bbox[1]], [bbox[2], bbox[3]]);
            let _ = tree.locate_in_envelope_intersecting_int(q, |c| {
                if c.idx > i {
                    out.push((i, c.idx));
                }
                core::ops::ControlFlow::<(), ()>::Continue(())
            });
        }
        out
    }
    #[cfg(not(feature = "rstar"))]
    {
        let mut out = Vec::new();
        for i in min_ring..n {
            for j in (i + 1)..n {
                if bbox_overlap(bboxes[i], bboxes[j]) {
                    out.push((i, j));
                }
            }
        }
        out
    }
}

/// Tree-accelerated exclusive point-in-ring (winding number, boundary =
/// outside), same arithmetic as point_in_ring_exclusive but only candidate
/// edges from the prebuilt shell tree are evaluated. The query box is the
/// +x ray slab [pt.x, max_x] x [pt.y +/- 1e-12]: every ray-crossing edge
/// has its x-max >= the crossing x >= pt.x and its y-span containing
/// pt.y, and every boundary candidate lies within 1e-12 of pt - so the
/// candidate set is a superset of the edges the original loop evaluates
/// for this point, and the result is identical.
#[cfg(feature = "rstar")]
fn point_in_ring_exclusive_tree(
    pt: Coord<f64>,
    shell: &[Coord<f64>],
    tree: &rstar::RTree<EdgeIdx>,
    max_x: f64,
) -> bool {
    let n = shell.len() - 1;
    if n < 2 {
        return false;
    }
    let pad = 1e-12;
    let q = rstar::AABB::from_corners([pt.x - pad, pt.y - pad], [max_x, pt.y + pad]);
    let mut boundary_hit = false;
    let mut wn = 0i32;
    for c in tree.locate_in_envelope_intersecting(q) {
        let p1 = shell[c.idx];
        let p2 = shell[(c.idx + 1) % n];
        let o = (p2.x - p1.x) * (pt.y - p1.y) - (p2.y - p1.y) * (pt.x - p1.x);
        if o.abs() < 1e-15 {
            let min_x = p1.x.min(p2.x);
            let max_x = p1.x.max(p2.x);
            let min_y = p1.y.min(p2.y);
            let max_y = p1.y.max(p2.y);
            if pt.x >= min_x - pad
                && pt.x <= max_x + pad
                && pt.y >= min_y - pad
                && pt.y <= max_y + pad
            {
                boundary_hit = true;
            }
        }
        if p1.y <= pt.y && p2.y > pt.y && o > 0.0 {
            wn += 1;
        } else if p2.y <= pt.y && o < 0.0 {
            wn -= 1;
        }
    }
    if boundary_hit {
        false
    } else {
        wn != 0
    }
}

/// Tree-accelerated hole-vs-shell crossing test: query the prebuilt shell
/// edge tree with each hole edge's envelope (GEOS-style indexed check; the
/// naive per-hole check_rings_intersect built a tree over the SMALL ring
/// and paid O(|shell| x log|hole|) per hole on giant shells).
#[cfg(feature = "rstar")]
fn ring_edges_intersect_tree(
    hole: &[Coord<f64>],
    shell: &[Coord<f64>],
    tree: &rstar::RTree<EdgeIdx>,
    eps: f64,
) -> bool {
    let n_hole = hole.len() - 1;
    let n_shell = shell.len() - 1;
    if n_hole == 0 || n_shell == 0 {
        return false;
    }
    for i in 0..n_hole {
        let a1 = hole[i];
        let a2 = hole[(i + 1) % n_hole];
        let (lo_x, hi_x) = if a1.x < a2.x { (a1.x, a2.x) } else { (a2.x, a1.x) };
        let (lo_y, hi_y) = if a1.y < a2.y { (a1.y, a2.y) } else { (a2.y, a1.y) };
        let q = rstar::AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]);
        let found = tree.locate_in_envelope_intersecting_int(q, |c| {
            let b1 = shell[c.idx];
            let b2 = shell[(c.idx + 1) % n_shell];
            if edges_intersect_general(a1, a2, b1, b2, eps) {
                core::ops::ControlFlow::Break(())
            } else {
                core::ops::ControlFlow::<(), ()>::Continue(())
            }
        });
        if found.is_break() {
            return true;
        }
    }
    false
}

/// Hole vertices lying EXACTLY on the shell boundary (vertex-on-edge and
/// shared vertices), via the prebuilt shell edge tree. Near-miss vertices
/// do NOT touch (GEOS exact-predicate parity; tolerance-based touches
/// fabricated contacts on real cadastral giants, measured 2026-08-06).
#[cfg(feature = "rstar")]
fn ring_touch_points(
    hole: &[Coord<f64>],
    shell: &[Coord<f64>],
    tree: &rstar::RTree<EdgeIdx>,
) -> Vec<Coord<f64>> {
    let n_shell = shell.len() - 1;
    let mut out = Vec::new();
    for &v in hole {
        let q = rstar::AABB::from_corners([v.x, v.y], [v.x, v.y]);
        let hit = tree
            .locate_in_envelope_intersecting_int(q, |c| {
                // Candidate edge bbox contains the vertex; the exact on-edge
                // test decides (a vertex on the edge's extension beyond its
                // endpoints is rejected by point_on_segment_exact).
                if point_on_segment_exact(v, shell[c.idx], shell[(c.idx + 1) % n_shell]) {
                    core::ops::ControlFlow::Break(())
                } else {
                    core::ops::ControlFlow::<(), ()>::Continue(())
                }
            })
            .is_break();
        if hit {
            out.push(v);
        }
    }
    out
}


/// Ring-touch cycle detection (extracted 2026-08-07; content verbatim).
mod cycle;
use crate::validation::holes::cycle::*;

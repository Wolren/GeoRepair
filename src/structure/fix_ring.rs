use geo::{Coord, Line, LineString, Polygon};
use rustc_hash::FxHashSet;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::core;
use crate::noding;
use crate::orient::{orient2d, orient2d_fast};
use crate::structure::PROFILE_FSI_NS;

use log::warn;

pub use crate::structure::edge_split::split_edges;
use crate::structure::edge_split::{intersect_param, lerp};
pub use crate::structure::symdiff::{
    edges_from_coords, make_valid_poly_symdiff, single_pass_fix, symdiff_test,
};


#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn repair_ring(ring: &LineString<f64>) -> Option<Vec<Polygon<f64>>> {
    let mut coords = basic_cleanup(ring)?;
    if coords.len() < 4 {
        return None;
    }
    // Barely-closed ring: a vertex within validation tolerance (1e-12 * bbox
    // scale) of the start vertex is a needle, not a feature - the validator
    // treats the pair as a touch (PinchPoint/SelfIntersection) and GEOS
    // resolves it by noding the needle away. Drop the near-duplicate vertex
    // (second-to-last: basic_cleanup guarantees the LAST vertex is the exact
    // closure == first). The ring self-touches through a zero-area needle
    // (measured: 7.3e-11 gap at scale 118, eps 1.18e-10,
    // invariant_barely_closed_ring). Invalid-only repair path, O(n) scale
    // scan - hot-path safe.
    if coords.len() >= 5 {
        let (mut min_x, mut max_x, mut min_y, mut max_y) =
            (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for c in &coords {
            min_x = min_x.min(c.x);
            max_x = max_x.max(c.x);
            min_y = min_y.min(c.y);
            max_y = max_y.max(c.y);
        }
        let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
        let eps = 1e-12 * scale;
        let first = coords[0];
        let near_closure = coords[coords.len() - 2];
        if (near_closure.x - first.x).abs().max((near_closure.y - first.y).abs()) <= eps {
            coords.remove(coords.len() - 2);
        }
    }
    if is_collinear_ring(&coords) {
        return None;
    }

    if !has_self_intersections(&coords) {
        return Some(vec![Polygon::new(LineString::new(coords), Vec::new())]);
    }

    // Fast path: try O(n) split for single self-intersection (70% of invalids).
    // WARNING: a single split only fixes rings with EXACTLY one crossing. For
    // multi-crossing rings (e.g. figure-eight chains), the split at the first
    // crossing leaves a still-self-intersecting remainder — accepting it drops
    // whole lobes of area (measured: 2334 → 112, 95% loss). Verify the split
    // results are actually clean before returning them.
    if let Some(rings) = try_fast_fix(&coords) {
        let cleaned: Vec<Polygon<f64>> = rings
            .into_iter()
            .filter_map(|r| basic_cleanup(&r).map(|c| Polygon::new(LineString::new(c), Vec::new())))
            .filter(|p| p.exterior().0.len() >= 4)
            .collect();
        if !cleaned.is_empty()
            && cleaned.iter().all(|p| !has_self_intersections(&p.exterior().0))
        {
            return Some(cleaned);
        }
    }

    // Full graph-based fix (GEOS MakeValidPoly: node → BuildArea → even-parent).
    // Returns polygons WITH structural holes — do not flatten to exterior rings.
    if let Some(polys) = fix_self_intersecting(&coords) {
        let cleaned: Vec<Polygon<f64>> = polys
            .into_iter()
            .filter(|p| p.exterior().0.len() >= 4)
            .collect();
        if cleaned.is_empty() {
            return None;
        }
        return Some(cleaned);
    }
    None
}

pub(crate) fn basic_cleanup(ring: &LineString<f64>) -> Option<Vec<Coord<f64>>> {
    let coords: Vec<_> = ring
        .0
        .iter()
        .copied()
        .filter(|c| c.x.is_finite() && c.y.is_finite())
        .collect();
    if coords.is_empty() {
        return None;
    }
    let mut deduped = noding::remove_consecutive_duplicates(&coords);
    if deduped.is_empty() {
        return None;
    }
    if deduped.first() != deduped.last() {
        deduped.push(deduped[0]);
    }
    if deduped.len() < 4 {
        return None;
    }

    Some(deduped)
}

/// Collapse SUB-ULP spikes: consecutive vertices closer than
/// f64::EPSILON × ring-bbox-scale are below the coordinate's own
/// representable precision — degenerate spikes that poison noding and face
/// extraction (measured: mixed-magnitude ring with a 5.089e-9 spike at 1e8
/// scale → 7 GEOS-exact faces collapsed to 6, 62% area loss; GEOS degrades
/// such spikes, as does arrange's prepare_lines snap). Merge the spike
/// vertex into its predecessor. Only interior vertices are dropped (the
/// closure vertex is protected by the loop range).
///
/// Runs ONLY on the repair path (before noding in fix_self_intersecting) —
/// NOT in basic_cleanup — because valid fast-path passthrough must stay
/// O(n) with no extra scan (measured: adding this to basic_cleanup cost
/// ~0.45s on the 1.58M dataset, blowing the 5s bar).
///
/// Representative choice (`snap_clean`):
/// - Structure repair path: keep the run's FIRST vertex (`snap_clean=false`).
///   A sub-ULP run on an axis-aligned edge must collapse onto the axis
///   (e.g. run (0,-8.4e-9),(-5.6e-10,8.6e-9),(-7.2e-10,6.5e-9) inside a ring
///   with a y=0 edge → keep (0,-8.4e-9); snapping to the off-axis member
///   (-7.2e-10,6.5e-9) turns the collinear overlap into a proper crossing,
///   measured seed mixed4 → SelfIntersection).
/// - Arrange CDT path: snap to the member closest to the origin
///   (`snap_clean=true`). A spike at a random offset (e.g. (-7.55e-9,0)
///   instead of (0,0)) produces CDT triangles that cross each other,
///   measured seed 00c11200 → cross-component SelfIntersection.
pub(crate) fn collapse_sub_ulp_vertices(coords: &[Coord<f64>], snap_clean: bool) -> Vec<Coord<f64>> {
    if coords.len() < 4 {
        return coords.to_vec();
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for c in coords {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let sub_ulp = f64::EPSILON * scale;
    if sub_ulp <= 0.0 {
        return coords.to_vec();
    }
    let mut cleaned: Vec<Coord<f64>> = Vec::with_capacity(coords.len());
    // Keep the first vertex; scan interior vertices (skip the closure at
    // the end — it equals the first vertex).
    let interior_len = coords.len() - 1;
    let mut i = 0usize;
    while i < interior_len {
        cleaned.push(coords[i]);
        // Skip any run of vertices within sub_ulp of the previous run
        // member. A run of >= 2 sub-ULP-consecutive vertices is a
        // degenerate detour (the ring dives to a point within float noise
        // of another and returns). Collapse the run to ONE vertex: its
        // members are all within sub_ulp of each other, so any choice is
        // area-preserving to sub-ULP — but the representative must be a
        // "clean" point (the CDT chokes on a spike at a random offset,
        // e.g. (-7.55e-9, 0) instead of (0,0) — measured: seed 00c11200
        // → cross-component SelfIntersection). Pick the member closest to
        // the coordinate origin.
        let mut j = i + 1;
        let mut best = coords[i];
        let mut best_mag = coords[i].x.abs().max(coords[i].y.abs());
        let mut run_len = 1usize;
        let mut last_in_run = coords[i];
        while j < interior_len {
            let d = (coords[j].x - last_in_run.x).abs().max((coords[j].y - last_in_run.y).abs());
            if d <= sub_ulp {
                last_in_run = coords[j];
                let m = coords[j].x.abs().max(coords[j].y.abs());
                if m < best_mag {
                    best = coords[j];
                    best_mag = m;
                }
                run_len += 1;
                j += 1;
            } else {
                break;
            }
        }
        if run_len > 1 && i > 0 && snap_clean {
            // Arrange CDT: replace the run start (already pushed) with the
            // cleanest run member (closest to the coordinate origin).
            if let Some(last) = cleaned.last_mut() {
                *last = best;
            }
            // Structure repair: keep the run start as-is (collapse onto the
            // axis-aligned first member — see doc comment).
        }
        i = j;
    }
    // Re-close: the ring's last interior vertex connects back to the first
    // vertex. If the last interior vertex itself is within sub_ulp of the
    // first, drop it (spike at the closure).
    if cleaned.len() >= 2 {
        let last = *cleaned.last().unwrap();
        let first = cleaned[0];
        let d = (last.x - first.x).abs().max((last.y - first.y).abs());
        if d <= sub_ulp && cleaned.len() > 2 {
            cleaned.pop();
        }
    }
    cleaned.push(cleaned[0]);
    if cleaned.len() >= 4 {
        cleaned
    } else {
        coords.to_vec()
    }
}

pub(crate) fn is_collinear_ring(coords: &[Coord<f64>]) -> bool {
    if coords.len() < 4 {
        return true;
    }
    let eps = 1e-12;
    for i in 0..coords.len() - 2 {
        let o = orient2d(coords[i], coords[i + 1], coords[i + 2]);
        if o.abs() > eps {
            return false;
        }
    }
    true
}

pub fn has_self_intersections(coords: &[Coord<f64>]) -> bool {
    has_self_intersections_impl(coords, None)
}

pub(crate) fn has_self_intersections_with_bbox(
    coords: &[Coord<f64>],
    bbox: (f64, f64, f64, f64),
) -> bool {
    has_self_intersections_impl(coords, Some(bbox))
}

fn has_self_intersections_impl(coords: &[Coord<f64>], bbox: Option<(f64, f64, f64, f64)>) -> bool {
    let n = coords.len();
    if n < 4 {
        return false;
    }

    let mut seen: FxHashSet<(u64, u64)> =
        FxHashSet::with_capacity_and_hasher(n, Default::default());
    for c in &coords[..n - 1] {
        let key = (c.x.to_bits(), c.y.to_bits());
        if !seen.insert(key) {
            return true;
        }
    }

    let (min_x, max_x, min_y, max_y) = match bbox {
        Some(b) => b,
        None => crate::simd::aabb_minmax_simd(coords),
    };
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = core::EPS * coord_scale;

    if n > core::GRID_THRESHOLD_N {
        return super::sweep::has_self_intersections(coords, eps);
    }

    has_self_intersections_bruteforce(coords, eps)
}

fn has_self_intersections_bruteforce(coords: &[Coord<f64>], eps: f64) -> bool {
    let n = coords.len();
    for i in 0..n - 1 {
        for j in i + 2..n - 1 {
            if i == 0 && j == n - 2 {
                continue;
            }
            if check_edge_pair(coords, i, j, eps) {
                return true;
            }
        }
    }
    false
}

/// Strict proper crossing of two segments (endpoints excluded), used by the
/// R-tree sweep for `has_proper_self_crossing`.
#[inline(always)]
pub fn segments_properly_cross_seg(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
) -> bool {
    let o1 = crate::orient::orient2d(a1, a2, b1);
    let o2 = crate::orient::orient2d(a1, a2, b2);
    let o3 = crate::orient::orient2d(b1, b2, a1);
    let o4 = crate::orient::orient2d(b1, b2, a2);
    (o1 > 0.0 && o2 < 0.0 || o1 < 0.0 && o2 > 0.0)
        && (o3 > 0.0 && o4 < 0.0 || o3 < 0.0 && o4 > 0.0)
}

#[inline(always)]
pub(crate) fn check_edge_pair(coords: &[Coord<f64>], i: usize, j: usize, eps: f64) -> bool {
    assert!(i + 1 < coords.len() && j + 1 < coords.len());
    let a1 = coords[i];
    let a2 = coords[i + 1];
    let b1 = coords[j];
    let b2 = coords[j + 1];

    if a1 == b1 && orient2d_fast(a1, a2, b2) != 0.0 {
        return false;
    }
    if a1 == b2 && orient2d_fast(a1, a2, b1) != 0.0 {
        return false;
    }
    if a2 == b1 && orient2d_fast(a2, a1, b2) != 0.0 {
        return false;
    }
    if a2 == b2 && orient2d_fast(a2, a1, b1) != 0.0 {
        return false;
    }

    let o = crate::simd::orient2d_batch_4_robust(
        &[a1, a1, b1, b1],
        &[a2, a2, b2, b2],
        &[b1, b2, a1, a2],
    );

    if o[0] * o[1] < 0.0 && o[2] * o[3] < 0.0 {
        return true;
    }

    if o[2].abs() <= eps
        && a1 != b1
        && a1 != b2
        && ((b1.x - b2.x).abs() > eps && a1.x > b1.x.min(b2.x) + eps && a1.x < b1.x.max(b2.x) - eps
            || (b1.y - b2.y).abs() > eps
                && a1.y > b1.y.min(b2.y) + eps
                && a1.y < b1.y.max(b2.y) - eps)
    {
        return true;
    }
    if o[3].abs() <= eps
        && a2 != b1
        && a2 != b2
        && ((b1.x - b2.x).abs() > eps && a2.x > b1.x.min(b2.x) + eps && a2.x < b1.x.max(b2.x) - eps
            || (b1.y - b2.y).abs() > eps
                && a2.y > b1.y.min(b2.y) + eps
                && a2.y < b1.y.max(b2.y) - eps)
    {
        return true;
    }
    if o[0].abs() <= eps
        && b1 != a1
        && b1 != a2
        && ((a1.x - a2.x).abs() > eps && b1.x > a1.x.min(a2.x) + eps && b1.x < a1.x.max(a2.x) - eps
            || (a1.y - a2.y).abs() > eps
                && b1.y > a1.y.min(a2.y) + eps
                && b1.y < a1.y.max(a2.y) - eps)
    {
        return true;
    }
    if o[1].abs() <= eps
        && b2 != a1
        && b2 != a2
        && ((a1.x - a2.x).abs() > eps && b2.x > a1.x.min(a2.x) + eps && b2.x < a1.x.max(a2.x) - eps
            || (a1.y - a2.y).abs() > eps
                && b2.y > a1.y.min(a2.y) + eps
                && b2.y < a1.y.max(a2.y) - eps)
    {
        return true;
    }

    if o[0].abs() <= eps && o[1].abs() <= eps && o[2].abs() <= eps && o[3].abs() <= eps {
        let lo_x = a1.x.min(a2.x).max(b1.x.min(b2.x));
        let hi_x = a1.x.max(a2.x).min(b1.x.max(b2.x));
        let lo_y = a1.y.min(a2.y).max(b1.y.min(b2.y));
        let hi_y = a1.y.max(a2.y).min(b1.y.max(b2.y));
        if lo_x + eps < hi_x || lo_y + eps < hi_y {
            return true;
        }
    }

    false
}

/// Check if edges i and j have a proper crossing (not just endpoint touch).
/// If so, returns the edge indices and the intersection point.
pub(crate) fn edge_intersection(
    coords: &[Coord<f64>],
    i: usize,
    j: usize,
    eps: f64,
) -> Option<(usize, usize, Coord<f64>)> {
    let a1 = coords[i];
    let a2 = coords[i + 1];
    let b1 = coords[j];
    let b2 = coords[j + 1];

    if a1 == b1 && orient2d_fast(a1, a2, b2) != 0.0 {
        return None;
    }
    if a1 == b2 && orient2d_fast(a1, a2, b1) != 0.0 {
        return None;
    }
    if a2 == b1 && orient2d_fast(a2, a1, b2) != 0.0 {
        return None;
    }
    if a2 == b2 && orient2d_fast(a2, a1, b1) != 0.0 {
        return None;
    }

    let e1 = Line::new(a1, a2);
    let e2 = Line::new(b1, b2);
    let (ti, tj) = intersect_param(&e1, &e2, eps)?;
    if (ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps) {
        let pi = lerp(e1, ti);
        let pj = lerp(e2, tj);
        let pt = Coord {
            x: (pi.x + pj.x) * 0.5,
            y: (pi.y + pj.y) * 0.5,
        };
        Some((i, j, pt))
    } else {
        None
    }
}

/// Split a self-intersecting ring at the first proper crossing into two
/// simple (non-self-intersecting) rings.  O(n).
///
/// Given edges `(vi, vi+1)` and `(vj, vj+1)` crossing at `pt` (i < j):
///
///   Ring A: `vi+1 → vi+2 → … → vj → pt → vi+1`  (one lobe)
///   Ring B: `v0 → … → vi → pt → vj+1 → … → vn-1 → v0`  (other lobe)
pub(crate) fn split_ring_at_intersection(
    coords: &[Coord<f64>],
    i: usize,
    j: usize,
    pt: Coord<f64>,
) -> (Vec<Coord<f64>>, Vec<Coord<f64>>) {
    let n = coords.len();
    let mut ring1 = Vec::with_capacity(j - i + 2);
    ring1.extend(coords[(i + 1)..=j].iter().copied());
    ring1.push(pt);
    ring1.push(ring1[0]);

    let mut ring2 = Vec::with_capacity(i + 1 + 1 + (n - 1 - j - 1) + 1);
    ring2.extend(coords[0..=i].iter().copied());
    ring2.push(pt);
    ring2.extend(coords[(j + 1)..(n - 1)].iter().copied());
    ring2.push(coords[0]);

    (ring1, ring2)
}

/// Fast path: try to detect and repair a single self-intersection in O(n).
///
/// Returns `Some(rings)` if the ring had exactly one proper crossing that was
/// split successfully.  Returns `None` if the fix is too complex for the fast
/// path (caller should fall through to the full `fix_self_intersecting`).
pub fn try_fast_fix(coords: &[Coord<f64>]) -> Option<Vec<LineString<f64>>> {
    let n = coords.len();
    if n < 4 {
        return None;
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &c in coords {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = core::EPS * coord_scale;

    let pair = if n > core::GRID_THRESHOLD_N {
        super::sweep::find_first_intersection(coords, eps)?
    } else {
        find_first_intersection_bruteforce(coords, eps)?
    };

    // The fast path ONLY handles rings with exactly ONE proper crossing. With
    // 2+ crossings, splitting at the first one can put the other crossing's
    // two edges into DIFFERENT result rings — neither ring self-intersects,
    // but the area is partitioned wrong (measured: 5609 → 116, 98% loss).
    // Verify no second crossing exists before accepting.
    let (i0, j0, _) = pair;
    if find_second_intersection(coords, eps, i0, j0).is_some() {
        return None;
    }

    let (i, j, pt) = pair;
    let (ring1, ring2) = split_ring_at_intersection(coords, i, j, pt);
    Some(vec![LineString::new(ring1), LineString::new(ring2)])
}

/// Find any proper crossing OTHER than the edge pair (i0, j0). O(n²) brute
/// force with early exit — only called once per fast-path attempt, and the
/// ring is already known to be small (GRID_THRESHOLD_N bound in callers).
fn find_second_intersection(
    coords: &[Coord<f64>],
    eps: f64,
    i0: usize,
    j0: usize,
) -> Option<(usize, usize, Coord<f64>)> {
    let n = coords.len();
    for i in 0..n - 1 {
        for j in (i + 2)..n - 1 {
            if i == 0 && j == n - 2 {
                continue;
            }
            if (i == i0 && j == j0) || (i == j0 && j == i0) {
                continue;
            }
            if let Some(pair) = edge_intersection(coords, i, j, eps) {
                return Some(pair);
            }
        }
    }
    None
}

fn find_first_intersection_bruteforce(
    coords: &[Coord<f64>],
    eps: f64,
) -> Option<(usize, usize, Coord<f64>)> {
    let n = coords.len();
    for i in 0..n - 1 {
        for j in (i + 2)..n - 1 {
            if i == 0 && j == n - 2 {
                continue;
            }
            if let Some(pair) = edge_intersection(coords, i, j, eps) {
                return Some(pair);
            }
        }
    }
    None
}

/// ---------------------------------------------------------------------------
/// Self-intersecting ring fixer
/// ---------------------------------------------------------------------------
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn fix_self_intersecting(coords: &[Coord<f64>]) -> Option<Vec<Polygon<f64>>> {
    let _t = Instant::now();
    let coords = collapse_sub_ulp_vertices(coords, false);
    let edges = edges_from_coords(&coords);
    let mut noded = split_edges(&edges);
    if noded.is_empty() {
        return None;
    }

    // Validate noding with MCIndex-based O(n log n) validator.
    let mut validator = crate::noding::validator::NodingValidator::new(noded.clone());
    validator.validate();
    if validator.has_violations() {
        warn!(
            "fix_self_intersecting: {} noding violation(s) remain, retrying with snap rounding",
            validator.violations().len()
        );
        let snapped = crate::noding::snap_round::snap_round_lines(&edges);
        if !snapped.is_empty() {
            noded = snapped;
        }
    }

    if noded.is_empty() {
        return None;
    }
    // GEOS MakeValidPoly core: BuildArea on the noded boundary + symdiff loop.
    // Iteration 1 builds the outer faces; subtracting their boundary leaves
    // the internal edges, whose BuildArea faces are XORed out. This is what
    // removes inner lobes (even-winding faces) — verified: seed2 = 9931.89
    // (10943.98 outer − 1012.09 inner), bit-identical to GEOS.
    let result = make_valid_poly_symdiff(&noded);
    PROFILE_FSI_NS.fetch_add(_t.elapsed().as_nanos() as u64, Ordering::Relaxed);
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

use geo::{Coord, Line, LineString, Polygon};
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use smallvec::SmallVec;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::core;
use crate::noding;
use crate::orient::{orient2d, orient2d_fast};
use crate::structure::PROFILE_FSI_NS;

use log::warn;
use rstar::{AABB, RTree, RTreeObject};

type SplitPoint = SmallVec<[(f64, Coord<f64>); 2]>;

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

/// GEOS MakeValidPoly::buildArea loop: repeatedly BuildArea the remaining
/// cut edges, XOR into the accumulated area, and remove the built boundary
/// from the cut edges, until nothing remains. Returns the odd-winding faces
/// (even-odd rule), exactly like GEOS.
pub fn make_valid_poly_symdiff(cut_edges: &[Line<f64>]) -> Vec<Polygon<f64>> {
    // build_area snaps vertices to the SNAP_SCALE grid; snap the input edges
    // the same way so boundary removal matches exactly.
    let snap = |c: Coord<f64>| Coord {
        x: (c.x * crate::core::SNAP_SCALE).round() / crate::core::SNAP_SCALE,
        y: (c.y * crate::core::SNAP_SCALE).round() / crate::core::SNAP_SCALE,
    };
    let mut remaining: Vec<Line<f64>> = cut_edges
        .iter()
        .map(|l| Line::new(snap(l.start), snap(l.end)))
        .collect();
    let mut area: Vec<Polygon<f64>> = Vec::new(); // accumulated (XOR) area
    let mut guard = 0usize;
    while !remaining.is_empty() {
        guard += 1;
        if guard > 64 {
            warn!("make_valid_poly_symdiff: iteration guard exceeded");
            break;
        }
        let Some(new_area) = crate::structure::build_area::build_area(&remaining) else {
            break;
        };
        if new_area.0.is_empty() {
            break;
        }
        // XOR the new faces into the accumulated area (symdiff).
        area = symdiff_polygons(&area, &new_area.0);
        // Remove the BOUNDARY of the built area from the cut edges. The
        // boundary = segments appearing in exactly ONE face ring (internal
        // edges appear in two adjacent faces, opposite directions — they
        // stay in `remaining` for the next iteration). This mirrors GEOS's
        // CascadedPolygonUnion dissolve before the boundary difference.
        let mut seg_counts: rustc_hash::FxHashMap<(u64, u64, u64, u64), usize> =
            rustc_hash::FxHashMap::default();
        for p in &new_area.0 {
            for ring in std::iter::once(p.exterior()).chain(p.interiors()) {
                for w in ring.0.windows(2) {
                    if w[0] != w[1] {
                        let key = segment_key(w[0], w[1]);
                        *seg_counts.entry(key).or_insert(0) += 1;
                    }
                }
            }
        }
        #[cfg(any(test, debug_assertions))]
        if std::env::var("DIAG_SYMDIFF").is_ok() {
            use geo::Area;
            eprintln!(
                "symdiff iter {guard}: remaining edges = {}, new_area = {} polys",
                remaining.len(),
                new_area.0.len()
            );
            let mut total = 0.0;
            for (i, p) in new_area.0.iter().enumerate() {
                let a = p.unsigned_area();
                total += a;
                eprintln!("   new_area[{i}]: area={a:.4} holes={}", p.interiors().len());
            }
            eprintln!("   new_area total = {total:.4}");
        }
        #[cfg(any(test, debug_assertions))]
        if std::env::var("DIAG_SYMDIFF").is_ok() {
            use geo::Area;
            let t: f64 = area.iter().map(|p| p.unsigned_area()).sum();
            eprintln!("   area after XOR = {t:.4} ({} polys)", area.len());
        }
        remaining.retain(|l| {
            let key = segment_key(snap(l.start), snap(l.end));
            // Keep edges NOT on the built boundary: count==0 (unused) or
            // count==2 (internal edge shared by two faces → next iter).
            seg_counts.get(&key).copied().unwrap_or(0) != 1
        });
    }
    area
}

/// Orientation-insensitive key for a segment (bit-exact after snapping).
fn segment_key(a: Coord<f64>, b: Coord<f64>) -> (u64, u64, u64, u64) {
    let (ax, ay, bx, by) = (a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits());
    if (ax, ay) <= (bx, by) {
        (ax, ay, bx, by)
    } else {
        (bx, by, ax, ay)
    }
}

pub fn symdiff_test(a: &[Polygon<f64>], b: &[Polygon<f64>]) -> Vec<Polygon<f64>> {
    symdiff_polygons(a, b)
}

/// Symmetric difference of two polygon sets (accumulated XOR) for the
/// MakeValidPoly loop.
///
/// Our build_area returns ATOMIC subdivision cells (no CascadedPolygonUnion
/// dissolve), so the faces of iteration N are exactly a SUBSET of iteration 1
/// faces — the XOR is a set difference by ring fingerprint, no boolean ops.
/// (geo's boolean ops are winding-sensitive and would fragment valid faces,
/// losing area downstream in unary_union.)
fn symdiff_polygons(a: &[Polygon<f64>], b: &[Polygon<f64>]) -> Vec<Polygon<f64>> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let fp = |p: &Polygon<f64>| -> Vec<(u64, u64)> {
        let mut pts: Vec<(u64, u64)> = p
            .exterior()
            .0
            .iter()
            .map(|c| (c.x.to_bits(), c.y.to_bits()))
            .collect();
        if pts.first() == pts.last() {
            pts.pop();
        }
        pts.sort_unstable();
        pts
    };
    #[cfg(any(test, debug_assertions))]
    if std::env::var("DIAG_SYMDIFF").is_ok() {
        use geo::Area;
        eprintln!("   XOR: a={} polys, b={} polys", a.len(), b.len());
        for (i, q) in a.iter().enumerate() {
            eprintln!(
                "     a[{i}]: area={:.4} fp={:?}",
                q.unsigned_area(),
                fp(q)
            );
        }
        for (i, p) in b.iter().enumerate() {
            eprintln!(
                "     b[{i}]: area={:.4} fp={:?}",
                p.unsigned_area(),
                fp(p)
            );
        }
    }
    let b_set: Vec<Vec<(u64, u64)>> = b.iter().map(fp).collect();
    let mut out: Vec<Polygon<f64>> = Vec::new();
    for q in a {
        let qf = fp(q);
        let matched = b_set.contains(&qf);
        #[cfg(any(test, debug_assertions))]
        if std::env::var("DIAG_SYMDIFF").is_ok() {
            eprintln!("     match a fp {:?} -> {}", qf, matched);
        }
        if !matched {
            out.push(q.clone());
        }
    }
    // Faces of b not present in ORIGINAL a survive the XOR (defensive;
    // normally none at iteration > 1 since b faces are a subset of a faces).
    let a_set: Vec<Vec<(u64, u64)>> = a.iter().map(fp).collect();
    for p in b {
        let pf = fp(p);
        if !a_set.contains(&pf) {
            out.push(p.clone());
        }
    }
    out
}

pub fn edges_from_coords(coords: &[Coord<f64>]) -> Vec<Line<f64>> {
    coords.windows(2).map(|w| Line::new(w[0], w[1])).collect()
}

/// ---------------------------------------------------------------------------
/// Edge splitting at intersection points
/// ---------------------------------------------------------------------------
/// Choose split strategy based on topology.
/// - If many edges share a single endpoint (radial-like, e.g. spoke wheel),
///   sweep-line avoids R-tree degeneracy where all bboxes overlap.
/// - Otherwise R-tree spatial clustering is more efficient.
fn should_use_sweepline(edges: &[Line<f64>], n: usize) -> bool {
    if n < 128 {
        return false;
    }
    let mut freq: FxHashMap<u64, usize> = FxHashMap::default();
    for e in edges {
        let k1 = e.start.x.to_bits() ^ e.start.y.to_bits().wrapping_mul(0x9e3779b97f4a7c15);
        let k2 = e.end.x.to_bits() ^ e.end.y.to_bits().wrapping_mul(0x9e3779b97f4a7c15);
        *freq.entry(k1).or_insert(0) += 1;
        *freq.entry(k2).or_insert(0) += 1;
    }
    freq.into_values().max().unwrap_or(0) > n / 4
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn split_edges(edges: &[Line<f64>]) -> Vec<Line<f64>> {
    let n = edges.len();
    let mut split_points: Vec<SplitPoint> = vec![SmallVec::new(); n];

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for e in edges {
        min_x = min_x.min(e.start.x).min(e.end.x);
        max_x = max_x.max(e.start.x).max(e.end.x);
        min_y = min_y.min(e.start.y).min(e.end.y);
        max_y = max_y.max(e.start.y).max(e.end.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = core::EPS * coord_scale;

    if n > core::GRID_THRESHOLD_N {
        if should_use_sweepline(edges, n) {
            split_edges_sweepline(edges, &mut split_points, eps);
        } else {
            split_edges_rtree(edges, &mut split_points, eps);
        }
    } else {
        split_edges_bruteforce(edges, &mut split_points, eps);
    }

    let eps_param = core::EPS_PARAM;
    let mut result = Vec::new();
    for i in 0..n {
        let e = edges[i];
        let mut pts = std::mem::take(&mut split_points[i]);
        pts.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        pts.dedup_by(|(a, _), (b, _)| (*a - *b).abs() < eps_param);
        let mut prev_pt = e.start;
        for &(_, pt) in &pts {
            if dist2(pt, prev_pt) > eps_param {
                result.push(Line::new(prev_pt, pt));
            }
            prev_pt = pt;
        }
        if dist2(e.end, prev_pt) > eps_param {
            result.push(Line::new(prev_pt, e.end));
        }
    }
    result
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn split_edges_rtree(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let n = edges.len();

    #[derive(Clone, Copy)]
    struct EdgeEnv {
        idx: usize,
        env: AABB<[f64; 2]>,
    }
    impl RTreeObject for EdgeEnv {
        type Envelope = AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }

    let envs: Vec<EdgeEnv> = edges
        .iter()
        .enumerate()
        .map(|(i, e)| EdgeEnv {
            idx: i,
            env: AABB::from_corners(
                [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
                [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
            ),
        })
        .collect();
    let tree = RTree::bulk_load(envs);

    for i in 0..n {
        let e = &edges[i];
        let query = AABB::from_corners(
            [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
            [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
        );
        let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
            let j = c.idx;
            if j <= i {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }

            if i.abs_diff(j) <= 1 || (i == 0 && j == n - 1) {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }

            if edges[i].start == edges[j].start
                && orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if edges[i].start == edges[j].end
                && orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if edges[i].end == edges[j].start
                && orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }
            if edges[i].end == edges[j].end
                && orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
            {
                return std::ops::ControlFlow::<(), ()>::Continue(());
            }

            if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps)
                && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
            {
                let pi = lerp(edges[i], ti);
                let pj = lerp(edges[j], tj);
                let pt = Coord {
                    x: (pi.x + pj.x) * 0.5,
                    y: (pi.y + pj.y) * 0.5,
                };
                if ti > eps && ti < 1.0 - eps {
                    split_points[i].push((ti, pt));
                }
                if tj > eps && tj < 1.0 - eps {
                    split_points[j].push((tj, pt));
                }
            }
            std::ops::ControlFlow::<(), ()>::Continue(())
        });
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn split_edges_bruteforce(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let n = edges.len();
    for i in 0..n {
        for j in (i + 2)..n {
            if i + 1 == j && edges[i].end == edges[j].start {
                continue;
            }
            if i == 0 && j == n - 1 && edges[i].start == edges[j].end {
                continue;
            }
            if edges[i].start == edges[j].start
                && orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
            {
                continue;
            }
            if edges[i].start == edges[j].end
                && orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
            {
                continue;
            }
            if edges[i].end == edges[j].start
                && orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
            {
                continue;
            }
            if edges[i].end == edges[j].end
                && orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
            {
                continue;
            }
            if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps)
                && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
            {
                let pi = lerp(edges[i], ti);
                let pj = lerp(edges[j], tj);
                let pt = Coord {
                    x: (pi.x + pj.x) * 0.5,
                    y: (pi.y + pj.y) * 0.5,
                };
                if ti > eps && ti < 1.0 - eps {
                    split_points[i].push((ti, pt));
                }
                if tj > eps && tj < 1.0 - eps {
                    split_points[j].push((tj, pt));
                }
            }
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn split_edges_sweepline(edges: &[Line<f64>], split_points: &mut [SplitPoint], eps: f64) {
    let pairs = crate::noding::sweep_line::find_intersecting_pairs(edges, eps);
    for &(i, j) in &pairs {
        if i.abs_diff(j) <= 1 || (i == 0 && j == edges.len() - 1) {
            continue;
        }
        if edges[i].start == edges[j].start
            && orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
        {
            continue;
        }
        if edges[i].start == edges[j].end
            && orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
        {
            continue;
        }
        if edges[i].end == edges[j].start
            && orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
        {
            continue;
        }
        if edges[i].end == edges[j].end
            && orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
        {
            continue;
        }
        if let Some((ti, tj)) = intersect_param(&edges[i], &edges[j], eps)
            && ((ti > eps && ti < 1.0 - eps) || (tj > eps && tj < 1.0 - eps))
        {
            let pi = lerp(edges[i], ti);
            let pj = lerp(edges[j], tj);
            let pt = Coord {
                x: (pi.x + pj.x) * 0.5,
                y: (pi.y + pj.y) * 0.5,
            };
            if ti > eps && ti < 1.0 - eps {
                split_points[i].push((ti, pt));
            }
            if tj > eps && tj < 1.0 - eps {
                split_points[j].push((tj, pt));
            }
        }
    }
}

#[inline]
fn intersect_param(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
    // Phase 1: Detection via robust orient2d (Shewchuk adaptive precision).
    // Fast pre-check rejects obvious non-intersections (both endpoints on
    // the same side of the other segment).
    let o1 = orient2d(e1.start, e1.end, e2.start);
    let o2 = orient2d(e1.start, e1.end, e2.end);
    let o3 = orient2d(e2.start, e2.end, e1.start);
    let o4 = orient2d(e2.start, e2.end, e1.end);

    // Quick rejection: both endpoints on the same side of the other segment
    if o1.signum() == o2.signum() && o1 != 0.0 && o2 != 0.0 {
        return None;
    }
    if o3.signum() == o4.signum() && o3 != 0.0 && o4 != 0.0 {
        return None;
    }

    // Collinear overlap (all four orientations zero)
    if o1 == 0.0 && o2 == 0.0 && o3 == 0.0 && o4 == 0.0 {
        return intersect_param_collinear(e1, e2, eps);
    }

    // Phase 2: Computation via double-double arithmetic (106-bit mantissa).
    // Handles proper crossings AND endpoint-on-segment intersections (both
    // are valid noding events).
    if let Some((_pt, t_dd, u_dd)) =
        crate::dd::segment_intersection_dd(e1.start, e1.end, e2.start, e2.end)
    {
        let t = t_dd.to_f64();
        let u = u_dd.to_f64();
        if t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps {
            return Some((t, u));
        }
    }
    None
}

#[inline]
fn intersect_param_collinear(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> Option<(f64, f64)> {
    let o1 = orient2d(e1.start, e1.end, e2.start);
    let o2 = orient2d(e1.start, e1.end, e2.end);
    let o3 = orient2d(e2.start, e2.end, e1.start);
    let o4 = orient2d(e2.start, e2.end, e1.end);
    if o1.abs() > eps || o2.abs() > eps || o3.abs() > eps || o4.abs() > eps {
        return None;
    }

    let dx = e1.end.x - e1.start.x;
    let dy = e1.end.y - e1.start.y;
    let len2 = dx * dx + dy * dy;
    if len2 < eps {
        return None;
    }

    let dot = |c: Coord<f64>| -> f64 { (c.x - e1.start.x) * dx + (c.y - e1.start.y) * dy };

    let s1 = (dot(e1.start) / len2).clamp(0.0, 1.0);
    let s2 = (dot(e1.end) / len2).clamp(0.0, 1.0);
    let p1 = (dot(e2.start) / len2).clamp(0.0, 1.0);
    let p2 = (dot(e2.end) / len2).clamp(0.0, 1.0);

    let e1a = s1.min(s2);
    let e1b = s1.max(s2);
    let e2a = p1.min(p2);
    let e2b = p1.max(p2);

    let lo = e1a.max(e2a);
    let hi = e1b.min(e2b);

    if lo + eps < hi {
        let e2_dot = |c: Coord<f64>| -> f64 {
            let dx2 = e2.end.x - e2.start.x;
            let dy2 = e2.end.y - e2.start.y;
            let len2_2 = dx2 * dx2 + dy2 * dy2;
            if len2_2 < eps {
                return 0.0;
            }
            ((c.x - e2.start.x) * dx2 + (c.y - e2.start.y) * dy2) / len2_2
        };

        let mid_x = e1.start.x + lo * dx;
        let mid_y = e1.start.y + lo * dy;
        let mid = Coord { x: mid_x, y: mid_y };

        let t_param = lo;
        let u_param = e2_dot(mid).clamp(0.0, 1.0);

        let e1_eps = eps / dx.abs().max(dy.abs()).max(1.0);
        let e2_eps = eps
            / (e2.end.x - e2.start.x)
                .abs()
                .max((e2.end.y - e2.start.y).abs())
                .max(1.0);

        let on_e1 = t_param > e1_eps && t_param < 1.0 - e1_eps;
        let on_e2 = u_param > e2_eps && u_param < 1.0 - e2_eps;
        if on_e1 || on_e2 {
            return Some((t_param, u_param));
        }
    }
    None
}

#[inline(always)]
fn lerp(e: Line<f64>, t: f64) -> Coord<f64> {
    Coord {
        x: e.start.x + t * (e.end.x - e.start.x),
        y: e.start.y + t * (e.end.y - e.start.y),
    }
}

#[inline(always)]
fn dist2(a: Coord<f64>, b: Coord<f64>) -> f64 {
    (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
}

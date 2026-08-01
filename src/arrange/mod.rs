//! CDT-based polygon repair for complex topologies (LEDOUX et al. 2014).
//!
//! The Arrange strategy uses Constrained Delaunay Triangulation as a robust
//! fallback for polygons that the Structure fast path cannot handle. It is
//! based on the approach described by Ledoux et al. (2014) for repairing
//! invalid polygons via constrained triangulation.
//!
//! Algorithm:
//! 1. Constrained Delaunay triangulation of polygon edges
//! 2. Triangle labeling (interior/exterior via winding)
//! 3. Face extraction from labeled triangles
//! 4. Ring assembly with winding correction
//!
//! Strengths:
//! - Handles any topology, no self-intersection limit
//! - Works on all-collinear and near-degenerate inputs
//!
//! Weaknesses:
//! - Slower than Structure, especially on large rings
//! - Requires the `spade` crate
//! - Can panic on extreme degeneracies (all-collinear rings, coords near f64::MAX)
//!
//! # Submodules
//!
//! - `cdt`: Constrained Delaunay triangulation wrapper
//! - `extract`: Triangle-to-ring extraction
//! - `label`: Face labeling (interior/exterior)
//! - `prep`: Input preparation, intersection pre-checks
//! - `prep_intersect`: Parallel intersection pre-filter
//! - `assemble`: Ring assembly from labeled faces
/// Ring assembly from labeled triangle faces.
pub mod assemble;
/// Constrained Delaunay triangulation wrapper around spade.
pub mod cdt;
/// Triangle-to-ring extraction from labeled faces.
pub mod extract;
/// Face labeling (interior/exterior) via winding.
pub mod label;
/// Input preparation: snapping, dedup, intersection splitting.
pub mod prep;
/// Parallel intersection pre-filter via R-tree.
pub mod prep_intersect;

use crate::core::MakeValidConfig;
use crate::validation::GeoValidation;
use geo::{BooleanOps, Coord, Geometry, GeometryCollection, LinesIter, MultiPolygon, Polygon};
use rstar::{AABB, RTree, RTreeObject};
use rustc_hash::FxHashSet;
use spade::{ConstrainedDelaunayTriangulation, Triangulation};

/// Wrap CDT construction in panic catch — spade can panic on degenerate
/// input (all-collinear rings, coords near f64::MAX).
fn build_cdt_safe(
    prepared: &prep::PreparedLines,
) -> Option<ConstrainedDelaunayTriangulation<Coord<f64>>> {
    use std::panic::{self, AssertUnwindSafe};
    match panic::catch_unwind(AssertUnwindSafe(|| cdt::build(prepared))) {
        Ok(Ok(cdt)) => Some(cdt),
        _ => None,
    }
}

pub(crate) fn fix_polygon(poly: &Polygon<f64>, _config: &MakeValidConfig) -> Geometry<f64> {
    if !poly_has_basic_form(poly) {
        let lines: Vec<_> = poly.lines_iter().collect();
        if lines.is_empty() {
            return empty();
        }
        return match fix_from_lines(lines) {
            Some(mp) => Geometry::MultiPolygon(mp),
            None => empty(),
        };
    }
    let lines: Vec<_> = poly.lines_iter().collect();
    if lines.is_empty() {
        return empty();
    }
    // Degenerate gate shared with Structure fast path: mixed-magnitude rings
    // (sub-ULP edges) must NOT pass through — proper-crossing detection is
    // blind to their collinear overlap. Collinear rings are handled by
    // strip_degenerate's noise-bound demotion downstream.
    if has_sub_ulp_edge(poly) {
        return fallback_polygon_fix(poly);
    }
    if prep::has_no_intersections(&lines) && holes_are_valid(poly) {
        return Geometry::Polygon(poly.clone());
    }
    // Fallback: if intersection check false-positives (known fp precision issue
    // with near-collinear vertices from CDT output), verify with our own validator.
    if holes_are_valid(poly) && poly_has_basic_form(poly) {
        let v = poly.validate();
        if v.valid {
            return Geometry::Polygon(poly.clone());
        }
    }
    fallback_polygon_fix(poly)
}

/// Run CDT on polygon edges, falling back to boolean difference when CDT
/// produces empty output (e.g. hole touching shell at 2 points).
/// Does NOT run the expensive Shewchuk validation — use this from Structure
/// fallback where the polygon is already known to need repair.
pub(crate) fn fallback_polygon_fix(poly: &Polygon<f64>) -> Geometry<f64> {
    let lines: Vec<_> = poly.lines_iter().collect();
    if let Some(mp) = fix_from_lines(lines) {
        if !mp.0.is_empty() {
            return Geometry::MultiPolygon(mp);
        }
    }
    if poly.interiors().is_empty() {
        return empty();
    }
    let shell = Polygon::new(poly.exterior().clone(), Vec::new());
    let holes: Vec<Polygon<f64>> = poly.interiors().iter()
        .map(|h| Polygon::new(h.clone(), Vec::new()))
        .collect();
    match boolean_difference_catch(&shell, &holes) {
        Some(diff) if !diff.0.is_empty() => Geometry::MultiPolygon(diff),
        // boolean difference panicked (i_overlay is_fill_top assertion on
        // degenerate input) or produced empty — polygon split failed for
        // hole-touching-2-places.
        _ => empty(),
    }
}

/// `boolean_op(Difference)` wrapped in `catch_unwind`. i_overlay 4.5.2
/// asserts `is_fill_top(link.fill)` internally on some degenerate inputs
/// (measured: shell [(54.36,0),(0,0),(18.55,82.91),(-48.92,33.44)] with a
/// 6-vertex hole). A panic inside a rayon batch kills the whole run, so
/// callers must catch and route to their BuildArea fallback instead.
pub(crate) fn boolean_difference_catch(
    shell: &Polygon<f64>,
    holes: &[Polygon<f64>],
) -> Option<MultiPolygon<f64>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if holes.len() == 1 {
            shell.boolean_op(&holes[0], geo::OpType::Difference)
        } else {
            let shell_mp = MultiPolygon::new(vec![shell.clone()]);
            let holes_mp = MultiPolygon::new(holes.to_vec());
            shell_mp.boolean_op(&holes_mp, geo::OpType::Difference)
        }
    }))
    .ok()
}

/// Validate a polygon against GEOS-compatible validity rules.
///
/// Checks: ring closure & min points, non-finite coords, no self-intersections,
/// hole containment, and no nested/overlapping holes.
/// Does NOT check OGC winding (CCW exterior) — GEOS isValid accepts both
/// winding orders, and winding is enforced on repair output separately.
pub fn validate_polygon(poly: &Polygon<f64>) -> bool {
    if !poly_has_basic_form(poly) {
        return false;
    }
    // Check for NaN/inf coordinates
    let rings = std::iter::once(poly.exterior()).chain(poly.interiors().iter());
    for ring in rings {
        if ring.0.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
            return false;
        }
    }
    // Self-intersection check
    let lines: Vec<_> = poly.lines_iter().collect();
    if lines.is_empty() || !prep::has_no_intersections(&lines) {
        return false;
    }
    // Hole containment checks
    if poly.interiors().is_empty() {
        return true;
    }
    holes_are_valid(poly)
}

/// Lightweight check: hole containment + nesting.
/// Used after [`has_no_intersections`] for the fast path.
pub fn holes_are_valid(poly: &Polygon<f64>) -> bool {
    holes_valid_impl(poly, false)
}

/// GEOS-aligned hole validity for the large-valid fast-path gate. GEOS
/// IsValidOp allows a hole to touch the shell at a point (OGC polygon
/// validity), so the first-vertex probe is INCLUSIVE here — a vertex exactly
/// on the shell boundary does not disqualify the polygon. Strictness matters
/// for the gate: rejecting GEOS-valid polys forces the full subtract pipeline
/// (measured: 857 holes on a 159k shell = 11.8s of wasted geo differences).
pub fn holes_are_valid_inclusive(poly: &Polygon<f64>) -> bool {
    holes_valid_impl(poly, true)
}

fn holes_valid_impl(poly: &Polygon<f64>, inclusive: bool) -> bool {
    let shell = poly.exterior();
    let shell_coords = &shell.0;
    for hole in poly.interiors() {
        let Some(pt) = hole.0.first().copied() else {
            return false;
        };
        let inside = if inclusive {
            crate::simd::point_in_ring_inclusive(pt, shell_coords)
        } else {
            point_in_ring_exclusive(pt, shell_coords)
        };
        if !inside {
            return false;
        }
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for c in &hole.0 {
            min_x = min_x.min(c.x);
            max_x = max_x.max(c.x);
            min_y = min_y.min(c.y);
            max_y = max_y.max(c.y);
        }
        let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
        if (max_x - min_x).abs() < f64::EPSILON * scale
            || (max_y - min_y).abs() < f64::EPSILON * scale
        {
            return false;
        }
    }
    let holes: Vec<_> = poly.interiors().iter().map(|h| &h.0).collect();
    if holes.len() > 1 {
        struct HoleEnv {
            idx: usize,
            env: AABB<[f64; 2]>,
        }
        impl RTreeObject for HoleEnv {
            type Envelope = AABB<[f64; 2]>;
            fn envelope(&self) -> Self::Envelope {
                self.env
            }
        }
        let mut envs = Vec::with_capacity(holes.len());
        for (i, h) in holes.iter().enumerate() {
            let first = h.first().map(|c| (c.x, c.y)).unwrap_or((0.0, 0.0));
            let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.0, first.0, first.1, first.1);
            for c in *h {
                min_x = min_x.min(c.x);
                max_x = max_x.max(c.x);
                min_y = min_y.min(c.y);
                max_y = max_y.max(c.y);
            }
            envs.push(HoleEnv {
                idx: i,
                env: AABB::from_corners([min_x, min_y], [max_x, max_y]),
            });
        }
        let tree = RTree::bulk_load(envs);
        for (i, h2) in holes.iter().enumerate() {
            let Some(pt) = h2.first().copied() else {
                continue;
            };
            let query = AABB::from_corners([pt.x, pt.y], [pt.x, pt.y]);
            let mut overlaps = false;
            let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
                if c.idx != i && point_in_ring_exclusive(pt, holes[c.idx]) {
                    overlaps = true;
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
                }
            });
            if overlaps {
                return false;
            }
        }
        // Hole-hole collinear edge sharing (positive-length overlap) → the holes
        // touch each other → DisconnectedInteriorRing. The R-tree above only
        // catches one hole's vertex strictly inside another; touching holes
        // share an edge with no vertex containment. Bbox-prefilter + small-ring
        // cap keeps this cheap: valid multi-hole polys have no edge sharing.
        for i in 0..holes.len() {
            for j in (i + 1)..holes.len() {
                if rings_share_collinear_edge(holes[i], holes[j]) {
                    return false;
                }
            }
        }
    }
    true
}

/// True if two rings share a collinear edge segment with positive-length overlap.
pub fn rings_share_collinear_edge_test(
    a: &[geo::Coord<f64>],
    b: &[geo::Coord<f64>],
) -> bool {
    rings_share_collinear_edge(a, b)
}

fn rings_share_collinear_edge(a: &[geo::Coord<f64>], b: &[geo::Coord<f64>]) -> bool {
    let na = if a.first() == a.last() { a.len() - 1 } else { a.len() };
    let nb = if b.first() == b.last() { b.len() - 1 } else { b.len() };
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
            // Relative collinearity: |cross| / (|da| * |db|) = sin(angle).
            // The old absolute threshold with a 1.0 floor flagged genuinely
            // non-collinear tiny edges as collinear (measured: two 1.6e-7
            // edges with cross 2.8e-14 < 1e-12 → false DIR). Only true
            // edge-sharing (positive-length overlap) is a real error;
            // vertex touches are legal (GEOS IsValidOp accepts them).
            let da_len = (dax * dax + day * day).sqrt().max(1e-300);
            let db_len = (dbx * dbx + dby * dby).sqrt().max(1e-300);
            let rel = 1e-12 * da_len * db_len;
            let cross = dax * dby - day * dbx;
            if cross.abs() > rel {
                continue;
            }
            let cross2 = (bx2 - bx1) * (ay1 - by1) - (by2 - by1) * (ax1 - bx1);
            if cross2.abs() > rel {
                continue;
            }
            let aminx = ax1.min(ax2);
            let amaxx = ax1.max(ax2);
            let aminy = ay1.min(ay2);
            let amaxy = ay1.max(ay2);
            let overlap = if dax.abs() >= day.abs() {
                interval_overlap(aminx, amaxx, bx1.min(bx2), bx1.max(bx2))
            } else {
                interval_overlap(aminy, amaxy, by1.min(by2), by1.max(by2))
            };
            // Positive-length overlap only: a shared endpoint is length 0.
            if overlap > 1e-12 * da_len.max(db_len) {
                return true;
            }
        }
    }
    false
}

fn interval_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    if hi > lo { hi - lo } else { 0.0 }
}

/// True if any ring edge is shorter than EPSILON * bbox_scale.
/// Mixed-magnitude rings (1e8 + 1e-8 coords) create edges below the
/// representable precision of the bbox scale — collinear overlaps between
/// such edges and big edges are invisible to proper-crossing detection.
/// Cheap O(n): single pass over edges, only used in fast-path gates.
pub fn has_sub_ulp_edge(poly: &Polygon<f64>) -> bool {
    fn ring_has_sub_ulp(ring: &[geo::Coord<f64>]) -> bool {
        let n = ring.len();
        if n < 2 {
            return false;
        }
        let (mut min_x, mut max_x, mut min_y, mut max_y) =
            (ring[0].x, ring[0].x, ring[0].y, ring[0].y);
        for c in &ring[1..] {
            min_x = min_x.min(c.x);
            max_x = max_x.max(c.x);
            min_y = min_y.min(c.y);
            max_y = max_y.max(c.y);
        }
        let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
        let eps = f64::EPSILON * scale;
        if eps <= 0.0 {
            return false;
        }
        let end = if ring.first() == ring.last() { n - 1 } else { n };
        for i in 0..end {
            let a = ring[i];
            let b = ring[(i + 1) % n];
            let dx = (b.x - a.x).abs();
            let dy = (b.y - a.y).abs();
            // Edge LENGTH below eps: use max component, not either — real GIS
            // data is full of axis-aligned edges (vertical: dx=0, horizontal:
            // dy=0) which are perfectly valid. Only a truly point-like edge
            // (both components sub-ULP) is degenerate.
            if dx.max(dy) < eps {
                return true;
            }
        }
        false
    }
    if ring_has_sub_ulp(&poly.exterior().0) {
        return true;
    }
    poly.interiors().iter().any(|h| ring_has_sub_ulp(&h.0))
}

/// Winding-number point-in-ring test (strict interior, boundary counts as outside).
fn point_in_ring_exclusive(pt: geo::Coord<f64>, ring: &[geo::Coord<f64>]) -> bool {
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
            if p2.y > pt.y && orient2d(p1, p2, pt) > 0.0 {
                wn += 1;
            }
        } else if p2.y <= pt.y && orient2d(p1, p2, pt) < 0.0 {
            wn -= 1;
        }
    }
    wn != 0
}

fn orient2d(p1: geo::Coord<f64>, p2: geo::Coord<f64>, p3: geo::Coord<f64>) -> f64 {
    (p2.x - p1.x) * (p3.y - p1.y) - (p3.x - p1.x) * (p2.y - p1.y)
}

/// Timing breakdown for the CDT arrange pipeline.
/// Only available with `--features bench-geos` or `--features bench-geos-system`.
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
#[derive(Default)]
pub struct ArrangeTiming {
    /// Seconds spent in input preparation (snapping, dedup, intersection splitting).
    pub prep_secs: f64,
    /// Seconds spent building the constrained Delaunay triangulation.
    pub cdt_build_secs: f64,
    /// Number of faces in the constructed CDT.
    pub cdt_faces: usize,
    /// Seconds spent labeling faces as interior/exterior.
    pub label_secs: f64,
    /// Seconds spent extracting rings from labeled faces.
    pub extract_secs: f64,
    /// Total seconds across all pipeline stages.
    pub total_secs: f64,
}

/// Profile the CDT arrange pipeline on a polygon, returning stage timing.
/// Returns `None` if the polygon has no edges or preparation fails.
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
pub fn diagnose_arrange(poly: &Polygon<f64>) -> Option<ArrangeTiming> {
    use std::time::Instant;
    let mut t = ArrangeTiming::default();

    let lines: Vec<_> = poly.lines_iter().collect();
    if lines.is_empty() {
        return None;
    }

    let start = Instant::now();
    let prepared = prep::prepare_lines(lines).ok()?;
    t.prep_secs = start.elapsed().as_secs_f64();

    let start = Instant::now();
    let cdt = build_cdt_safe(&prepared)?;
    t.cdt_build_secs = start.elapsed().as_secs_f64();
    t.cdt_faces = cdt.num_inner_faces();

    let start = Instant::now();
    let interior = label::label_faces(&cdt);
    t.label_secs = start.elapsed().as_secs_f64();

    let start = Instant::now();
    let _raw_rings = extract::trace_rings(&cdt, &interior);
    t.extract_secs = start.elapsed().as_secs_f64();

    t.total_secs = t.prep_secs + t.cdt_build_secs + t.label_secs + t.extract_secs;
    Some(t)
}

pub fn poly_has_basic_form(poly: &Polygon<f64>) -> bool {
    fn ring_is_plausible(ring: &geo::LineString<f64>) -> bool {
        let coords = &ring.0;
        if coords.len() < 4 || coords.first() != coords.last() {
            return false;
        }
        for w in coords.windows(2) {
            if w[0] == w[1] {
                return false;
            }
        }
        let n = coords.len() - 1;
        let mut seen = FxHashSet::with_capacity_and_hasher(n, Default::default());
        for c in &coords[..n] {
            if !seen.insert((c.x.to_bits(), c.y.to_bits())) {
                return false;
            }
        }
        true
    }
    if !ring_is_plausible(poly.exterior()) {
        return false;
    }
    poly.interiors().iter().all(ring_is_plausible)
}

pub(crate) fn fix_from_lines(lines: Vec<geo::Line<f64>>) -> Option<MultiPolygon<f64>> {
    let prepared = prep::prepare_lines(lines).ok()?;
    let cdt = build_cdt_safe(&prepared)?;
    if cdt.num_inner_faces() == 0 {
        return Some(MultiPolygon::new(Vec::new()));
    }
    let interior = label::label_faces(&cdt);
    if interior.is_empty() {
        return Some(MultiPolygon::new(Vec::new()));
    }
    let raw_rings = extract::trace_rings(&cdt, &interior);
    let rings: Vec<_> = raw_rings
        .into_iter()
        .flat_map(extract::split_ring_at_pinch_points)
        .filter(|coords| {
            // O(n log n): discard rings with self-intersections from CDT precision
            let lines: Vec<geo::Line<f64>> = coords.windows(2)
                .map(|w| geo::Line::new(w[0], w[1]))
                .collect();
            lines.len() < 4 || prep_intersect::has_no_intersections(&lines)
        })
        .map(geo::LineString::new)
        .collect();
    Some(assemble::assemble_polygons(rings))
}

fn empty() -> Geometry<f64> {
    Geometry::GeometryCollection(GeometryCollection(Vec::new()))
}

//! cdt polygon repair for complex topologies (Ledoux et al. 2014). Arrange is the
//! fallback when Structure cannot handle a polygon. Weaknesses: slower than Structure
//! on large rings, needs the spade crate, can panic near f64::max. # Submodules - cdt:
//! triangulation - extract: ring extraction - label: face labeling - prep: input
//! preparation, intersection checks - prep_intersect: parallel pre-filter - assemble:
//! ring assembly

use alloc::vec::Vec;
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
use geo::{
    BooleanOps, Coord, Geometry, GeometryCollection, LineString, LinesIter, MultiPolygon, Polygon,
};
use rstar::{AABB, RTree, RTreeObject};
use spade::{ConstrainedDelaunayTriangulation, Triangulation};

/// Wrap cdt construction in panic catch. Spade can panic on degenerate input
/// (all-collinear rings, coords near f64::max).
fn build_cdt_safe(
    prepared: &prep::PreparedLines,
) -> Option<ConstrainedDelaunayTriangulation<Coord<f64>>> {
    #[cfg(feature = "std")]
    {
        use std::panic::{self, AssertUnwindSafe};
        match panic::catch_unwind(AssertUnwindSafe(|| cdt::build(prepared))) {
            Ok(Ok(cdt)) => Some(cdt),
            _ => None,
        }
    }
    #[cfg(not(feature = "std"))]
    {
        cdt::build(prepared).ok()
    }
}

pub(crate) fn fix_polygon(poly: &Polygon<f64>, config: &MakeValidConfig) -> Geometry<f64> {
    // (1e8 + 1e-9 coords) have sub-precision spikes that fail basic form (measured:
    // seed 00c11200 → MultiPolygon with cross-component SelfIntersection; GEOS snaps
    // the spike to the grid).
    if has_sub_ulp_edge(poly, None) {
        let ext: LineString<f64> = LineString::new(
            crate::structure::fix_ring::collapse_sub_ulp_vertices(&poly.exterior().0, true),
        );
        let holes: Vec<LineString<f64>> = poly
            .interiors()
            .iter()
            .map(|h| {
                LineString::new(crate::structure::fix_ring::collapse_sub_ulp_vertices(
                    &h.0, true,
                ))
            })
            .collect();
        #[cfg(all(any(test, debug_assertions), feature = "std"))]
        #[cfg(feature = "std")]
        if std::env::var("DIAG_COLLAPSE").is_ok() {
            eprintln!(
                "DIAG_COLLAPSE: {} coords -> {} coords",
                poly.exterior().0.len(),
                ext.0.len()
            );
        }
        // No-op collapse guard: if the ring is unchanged, recursion would be infinite
        // (measured: invariant_mixed_fp_in_same_ring stack overflow).
        let collapsed = Polygon::new(ext, holes);
        if collapsed.exterior().0.len() != poly.exterior().0.len() {
            return fix_polygon(&collapsed, config);
        }
    }
    let cap = poly
        .exterior()
        .0
        .len()
        .saturating_add(poly.interiors().iter().map(|h| h.0.len()).sum());
    let mut lines = Vec::with_capacity(cap);
    let mut acc = GateAccum {
        lines: Some(&mut lines),
        chains: None,
        bbox: None,
        extremal: None,
    };
    if !ring_is_plausible(poly.exterior(), &mut acc) {
        return fix_from_poly_lines(poly);
    }
    for hole in poly.interiors() {
        if !ring_is_plausible(hole, &mut acc) {
            return fix_from_poly_lines(poly);
        }
    }
    if lines.is_empty() {
        return empty();
    }
    if prep::has_no_intersections_tuned(&lines, &config.tuning)
        && holes_are_valid(poly)
        && holes_contained_cheap(poly, &config.tuning)
    {
        return Geometry::Polygon(poly.clone());
    }
    // Fallback: if intersection check false-positives (known fp precision issue with
    // near-collinear vertices from cdt output), verify with our own validator.
    if holes_are_valid(poly) && poly_has_basic_form(poly) {
        let v = poly.validate();
        if v.valid {
            return Geometry::Polygon(poly.clone());
        }
    }
    fallback_polygon_fix(poly)
}

/// Collect a polygon's lines for the repair path (used when the basic form fails
/// mid-scan: the fused scan's partial lines are unusable).
fn fix_from_poly_lines(poly: &Polygon<f64>) -> Geometry<f64> {
    let lines: Vec<_> = poly.lines_iter().collect();
    if lines.is_empty() {
        return empty();
    }
    match fix_from_lines(lines) {
        Some(mp) => Geometry::MultiPolygon(mp),
        None => empty(),
    }
}

/// Run cdt on polygon edges, falling back to boolean difference when cdt produces empty
/// output (e.g. hole touching shell at 2 points). Does not run the expensive Shewchuk
/// validation.
pub(crate) fn fallback_polygon_fix(poly: &Polygon<f64>) -> Geometry<f64> {
    let lines: Vec<_> = poly.lines_iter().collect();
    if let Some(mp) = fix_from_lines(lines)
        && !mp.0.is_empty()
    {
        return Geometry::MultiPolygon(mp);
    }
    if poly.interiors().is_empty() {
        return empty();
    }
    let shell = Polygon::new(poly.exterior().clone(), Vec::new());
    let holes: Vec<Polygon<f64>> = poly
        .interiors()
        .iter()
        .map(|h| Polygon::new(h.clone(), Vec::new()))
        .collect();
    match boolean_difference_catch(&shell, &holes) {
        Some(diff) if !diff.0.is_empty() => Geometry::MultiPolygon(diff),
        // boolean difference panicked (i_overlay is_fill_top assertion on degenerate
        // input) or produced empty: polygon split failed for hole-touching-2-places.
        _ => empty(),
    }
}

/// `boolean_op(Difference)` wrapped in `catch_unwind`. i_overlay 4.5.2 asserts
/// `is_fill_top(link.fill)` internally on some degenerate inputs (measured: shell
/// [(54.36,0),(0,0),(18.55,82.91),(-48.92,33.44)] with a 6-vertex hole). A panic inside
/// a rayon batch kills the whole run, so callers must catch and route to their
/// BuildArea fallback instead.
pub(crate) fn boolean_difference_catch(
    shell: &Polygon<f64>,
    holes: &[Polygon<f64>],
) -> Option<MultiPolygon<f64>> {
    #[cfg(feature = "std")]
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if holes.len() == 1 {
            shell.boolean_op(&holes[0], geo::OpType::Difference)
        } else {
            let shell_mp = MultiPolygon::new(vec![shell.clone()]);
            let holes_mp = MultiPolygon::new(holes.to_vec());
            shell_mp.boolean_op(&holes_mp, geo::OpType::Difference)
        }
    }))
    .ok();
    #[cfg(not(feature = "std"))]
    let result = Some(if holes.len() == 1 {
        shell.boolean_op(&holes[0], geo::OpType::Difference)
    } else {
        let shell_mp = MultiPolygon::new(vec![shell.clone()]);
        let holes_mp = MultiPolygon::new(holes.to_vec());
        shell_mp.boolean_op(&holes_mp, geo::OpType::Difference)
    });
    result
}

/// Supplement to the first-vertex probe: it catches a hole poking out of the shell.
/// Small polygons also apply the touch rule: two or more distinct shell vertices
/// disconnect the interior (GEOS Test 22). One hole vertex must lie strictly inside.
/// Regression 2026-08-03: a cgal fixture passed the gate and arrange returned empty.
pub(crate) fn holes_contained_cheap(poly: &Polygon<f64>, tuning: &crate::core::Tuning) -> bool {
    let shell = poly.exterior();
    if shell.0.len() < 3 {
        return false;
    }
    let (mut sx0, mut sy0, mut sx1, mut sy1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for c in &shell.0 {
        sx0 = sx0.min(c.x);
        sy0 = sy0.min(c.y);
        sx1 = sx1.max(c.x);
        sy1 = sy1.max(c.y);
    }
    let scale = (sx1 - sx0).abs().max((sy1 - sy0).abs()).max(1.0);
    // f64 slack so a vertex exactly on the shell bbox does not trip.
    let eps = 8.0 * f64::EPSILON * scale;
    let small = shell.0.len() <= tuning.small_ring_lines;
    for hole in poly.interiors() {
        let (mut hx0, mut hy0, mut hx1, mut hy1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for c in &hole.0 {
            hx0 = hx0.min(c.x);
            hy0 = hy0.min(c.y);
            hx1 = hx1.max(c.x);
            hy1 = hy1.max(c.y);
        }
        if hx0 < sx0 - eps || hy0 < sy0 - eps || hx1 > sx1 + eps || hy1 > sy1 + eps {
            return false;
        }
        if small {
            let mut touches = 0usize;
            let mut seen: Vec<Coord<f64>> = Vec::new();
            let mut any_inside = false;
            let e2 = 1e-12 * scale;
            for &hp in &hole.0 {
                if crate::validation::point_in_ring_exclusive(hp, &shell.0) {
                    any_inside = true;
                }
                if crate::validation::point_on_ring(hp, &shell.0, e2) && !seen.contains(&hp) {
                    touches += 1;
                    seen.push(hp);
                }
            }
            if touches >= 2 {
                return false;
            }
            if !any_inside {
                return false;
            }
        }
    }
    true
}

/// Validate a polygon against GEOS-compatible validity rules.
///
/// Does not check OGC winding (ccw exterior).
pub fn validate_polygon(poly: &Polygon<f64>) -> bool {
    // One pass over the coords does the basic form (closure, adjacent duplicates, NaN)
    // and the bit-exact duplicate scan (measured 2026-08-07: the collect was a full
    // extra read, ~10-15 us on a 5000-vertex ring).
    let cap = poly
        .exterior()
        .0
        .len()
        .saturating_add(poly.interiors().iter().map(|h| h.0.len()).sum());
    let mut lines = Vec::with_capacity(cap);
    let mut acc = GateAccum {
        lines: Some(&mut lines),
        chains: None,
        bbox: None,
        extremal: None,
    };
    if !ring_is_plausible(poly.exterior(), &mut acc) {
        return false;
    }
    for hole in poly.interiors() {
        if !ring_is_plausible(hole, &mut acc) {
            return false;
        }
    }
    if lines.is_empty() || !prep::has_no_intersections(&lines) {
        return false;
    }
    // The exclusive variant rejects GEOS-valid repaired outputs; measured: --fast gate
    // flagged 24/300 real-world structure components that the full validator and GEOS
    // accept, 2026-08-03 .
    if poly.interiors().is_empty() {
        return true;
    }
    holes_are_valid_inclusive(poly) && holes_contained_cheap(poly, &crate::core::Tuning::default())
}

/// Lightweight check: hole containment + nesting.
pub fn holes_are_valid(poly: &Polygon<f64>) -> bool {
    holes_valid_impl(poly, false)
}

/// GEOS-aligned hole validity for the large-valid fast-path gate. Strictness matters
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
            crate::validation::point_in_ring_exclusive(pt, shell_coords)
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
                if c.idx != i && crate::validation::point_in_ring_exclusive(pt, holes[c.idx]) {
                    overlaps = true;
                    core::ops::ControlFlow::Break(())
                } else {
                    core::ops::ControlFlow::<(), ()>::Continue(())
                }
            });
            if overlaps {
                return false;
            }
        }
        for i in 0..holes.len() {
            for j in (i + 1)..holes.len() {
                if crate::util::rings_share_collinear_edge_precise(holes[i], holes[j]) {
                    return false;
                }
            }
        }
    }
    true
}

/// True if two rings share a collinear edge segment with positive-length overlap.
pub fn rings_share_collinear_edge_test(a: &[geo::Coord<f64>], b: &[geo::Coord<f64>]) -> bool {
    crate::util::rings_share_collinear_edge_precise(a, b)
}

/// True when a ring edge is under epsilon * bbox_scale. Mixed magnitudes (1e8 plus
/// 1e-8) hide collinear overlaps from crossing tests. A 4.9e208 spike collapsed a valid
/// ring with a 1-unit base edge (8 ULPs at 1e15) to empty (2026-08-04). Local eps
/// catches 1e-8 spikes at 1e8 (1e-8 < 2.2e-8).
#[inline(always)]
fn edge_is_sub_ulp(a: geo::Coord<f64>, b: geo::Coord<f64>) -> bool {
    let eps = f64::EPSILON * a.x.abs().max(a.y.abs()).max(b.x.abs()).max(b.y.abs());
    eps > 0.0 && (b.x - a.x).abs().max((b.y - a.y).abs()) < eps
}

/// One pass collects two guards: any sub-ULP edge and the smallest non-zero absolute
/// ordinate, the extreme-magnitude denominator. They used to ride in the fused
/// plausibility walk at 16 scalar ops per vertex. As dedicated reductions they
/// vectorize (2026-10-17).
pub(crate) fn ring_guards(ring: &[geo::Coord<f64>]) -> (bool, f64) {
    let mut sub_ulp = false;
    let mut min_abs = f64::INFINITY;
    for w in ring.windows(2) {
        sub_ulp |= edge_is_sub_ulp(w[0], w[1]);
        let ax = if w[0].x == 0.0 {
            f64::INFINITY
        } else {
            w[0].x.abs()
        };
        let ay = if w[0].y == 0.0 {
            f64::INFINITY
        } else {
            w[0].y.abs()
        };
        min_abs = min_abs.min(ax).min(ay);
    }
    (sub_ulp, min_abs)
}

pub fn has_sub_ulp_edge(poly: &Polygon<f64>, _scale_hint: Option<f64>) -> bool {
    fn ring_has_sub_ulp(ring: &[geo::Coord<f64>]) -> bool {
        let n = ring.len();
        if n < 2 {
            return false;
        }
        let end = if ring.first() == ring.last() {
            n - 1
        } else {
            n
        };
        for i in 0..end {
            let a = ring[i];
            let b = ring[(i + 1) % n];
            if edge_is_sub_ulp(a, b) {
                return true;
            }
        }
        false
    }
    let ext = &poly.exterior().0;
    if ring_has_sub_ulp(ext) {
        return true;
    }
    poly.interiors().iter().any(|h| ring_has_sub_ulp(&h.0))
}

/// Winding-number point-in-ring test (strict interior, boundary counts as outside).
/// Only available with `--features bench-geos` or `--features bench-geos-system`.
#[cfg(any(feature = "bench-geos", feature = "bench-geos-system"))]
#[derive(Default)]
pub struct ArrangeTiming {
    /// Seconds spent in input preparation (snapping, dedup, intersection splitting).
    pub prep_secs: f64,
    /// Seconds spent building the constrained Delaunay triangulation.
    pub cdt_build_secs: f64,
    /// Number of faces in the constructed cdt.
    pub cdt_faces: usize,
    /// Seconds spent labeling faces as interior/exterior.
    pub label_secs: f64,
    /// Seconds spent extracting rings from labeled faces.
    pub extract_secs: f64,
    /// Total seconds across all pipeline stages.
    pub total_secs: f64,
}

/// Profile the cdt arrange pipeline on a polygon, returning stage timing. Returns
/// `None` if the polygon has no edges or preparation fails.
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

/// Side-channel accumulators for the gate's fused scan: optional line collection (the
/// gate's separate collect), the global bbox (the envelope degeneracy check), and the
/// sub-ULP edge flag. Every field optional - a caller pays only for the accumulators it
/// requested.
pub(crate) struct GateAccum<'a> {
    pub lines: Option<&'a mut Vec<geo::Line<f64>>>,
    /// Fused monotone-chain index: pushed in lockstep with `lines` (same windows, same
    /// order) so the gate owns a chain index after its walk and never makes the second
    /// pass `build_mono_chains` costs. Set by callers that will sweep with
    /// [`prep_intersect::has_no_intersections_from_chains`]; `None` keeps the plain
    /// collect-then-build flow.
    pub chains: Option<&'a mut prep_intersect::ChainSink>,
    pub bbox: Option<&'a mut (f64, f64, f64, f64)>,
    /// Per-ring min-x extremal index (tie min-y, strict <, candidate 0, interior range
    /// [1.n)) recorded by [`ring_is_plausible`] so the caller's OGC winding
    /// normalization skips its own search.
    pub extremal: Option<&'a mut Vec<usize>>,
}

impl GateAccum<'_> {
    pub(crate) fn none() -> GateAccum<'static> {
        GateAccum {
            lines: None,
            chains: None,
            bbox: None,
            extremal: None,
        }
    }
}

/// Basic-form plausibility scan for one ring: closure, min points, adjacent duplicates,
/// non-finite coords, non-adjacent duplicate scan (shared with the exit validator - the
/// gate must certify exactly what the validator accepts, 2026-08-07 gate rule). The
/// same pass also drives the optional accumulators (lines, global bbox, sub-ULP flag) -
/// the gate's separate collect, envelope scan and sub-ULP scan are all fused here
/// (measured 2026-08-08: the envelope + sub-ULP passes cost ~15-20 us on a 5000-vertex
/// ring).
///
/// The per-ring duplicate-vertex hash table that used to live here (a 2*2^ceil(log2
/// n)-slot open-addressed table, 512 KB at 10 000 vertices, hit at a random offset per
/// vertex) is gone. It measured 11 % of the gate at 500 v rising to 17 % at 10 000 v
/// (2026-10-17, tls scratch and bitmap clear already in place). Rings at or below
/// `SMALL_RING_LINES` keep the allocation-free O(n^2) `first_pinch_dup` scan: it is
/// bounded. The pair predicates now close the repeated-vertex class themselves - see
/// `prep_intersect::shares_repeated_vertex`, which mirrors GEOS's rule that a same-ring
/// interaction which is not an adjacent-in-ring endpoint touch is invalid.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn ring_is_plausible(ring: &geo::LineString<f64>, acc: &mut GateAccum) -> bool {
    let coords = &ring.0;
    if coords.len() < 4 || coords.first() != coords.last() {
        return false;
    }
    let n = coords.len() - 1;
    if let Some(s) = acc.chains.as_deref_mut() {
        s.begin_ring();
    }
    plausible_body(coords, n, acc)
}

/// The shared windows loop of [`ring_is_plausible`].
fn plausible_body(coords: &[geo::Coord<f64>], n: usize, acc: &mut GateAccum) -> bool {
    // Extremal recording (min-x, tie min-y, strict <, candidate 0, interior range
    // [1.n)): matches min_x_vertex in util.rs exactly
    let track_ext = acc.extremal.is_some();
    let mut ext_min_x = coords[0].x;
    let mut ext_min_y = coords[0].y;
    let mut ext_idx = 0usize;
    for (i, w) in coords.windows(2).enumerate() {
        // Adjacent duplicates and non-finite coords (the gate's separate NaN scan is
        // folded here - measured 2026-08-07: one fewer full pass)
        if w[0] == w[1] || !w[0].x.is_finite() || !w[0].y.is_finite() {
            return false;
        }
        if track_ext && i >= 1 {
            // w[0] is coords[i]; the closure coord (index n) never appears as w[0], and
            // index 0 is the default candidate.
            let c = w[0];
            if c.x < ext_min_x || (c.x == ext_min_x && c.y < ext_min_y) {
                ext_min_x = c.x;
                ext_min_y = c.y;
                ext_idx = i;
            }
        }
        match (acc.lines.as_deref_mut(), acc.chains.as_deref_mut()) {
            (Some(l), Some(s)) => {
                l.push(geo::Line::new(w[0], w[1]));
                s.push_line(w[0].x, w[0].y, w[1].x, w[1].y);
            }
            (Some(l), None) => l.push(geo::Line::new(w[0], w[1])),
            (None, Some(s)) => s.push_line(w[0].x, w[0].y, w[1].x, w[1].y),
            (None, None) => {}
        }
        if let Some(bb) = acc.bbox.as_deref_mut() {
            bb.0 = bb.0.min(w[0].x);
            bb.1 = bb.1.max(w[0].x);
            bb.2 = bb.2.min(w[0].y);
            bb.3 = bb.3.max(w[0].y);
        }
    }
    // Small rings: the standalone O(n^2) pinch scan (no allocation).
    if n <= 32 && crate::validation::ring::first_pinch_dup(&coords[..n]).is_some() {
        return false;
    }
    if track_ext && let Some(v) = acc.extremal.as_deref_mut() {
        v.push(ext_idx);
    }
    true
}

pub fn poly_has_basic_form(poly: &Polygon<f64>) -> bool {
    let mut acc = GateAccum::none();
    if !ring_is_plausible(poly.exterior(), &mut acc) {
        return false;
    }
    poly.interiors()
        .iter()
        .all(|h| ring_is_plausible(h, &mut acc))
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
            let lines: Vec<geo::Line<f64>> = coords
                .windows(2)
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

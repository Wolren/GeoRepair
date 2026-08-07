//! per-geometry GeoValidation impls and free validate functions
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance:
//! core.rs was 2540 lines; the cap is 800). Content is verbatim - no
//! behavior changes; sibling modules resolve shared items through the
//! `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.



use alloc::vec::Vec;
use alloc::string::String;
use crate::validation::core::*;
use geo::{Coord, Line, LineString, MultiLineString, MultiPoint, Point, Rect, Triangle};

impl GeoValidation for Point<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        // Point(NaN, NaN) is the geo representation of POINT EMPTY - valid OGC.
        if self.x().is_nan() && self.y().is_nan() {
            return ValidationResult::valid();
        }
        // A point with a non-finite ordinate (NaN or inf) is invalid -
        // GEOS reports "Invalid Coordinate" (verified: TestValid NaN cases
        // expect isValid=false).
        if !self.x().is_finite() || !self.y().is_finite() {
            return ValidationResult::invalid(vec![GeometryValidationError::CoordinateNaN]);
        }
        ValidationResult::valid()
    }
}

impl GeoValidation for MultiPoint<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        for p in &self.0 {
            let r = p.validate();
            if !r.valid {
                errors.extend(r.errors);
            }
        }
        // OGC Simple Features: MultiPoint must not contain duplicate points
        if self.0.len() > 1 {
            let mut seen: rustc_hash::FxHashSet<(u64, u64)> =
                rustc_hash::FxHashSet::with_capacity_and_hasher(self.0.len(), Default::default());
            for p in &self.0 {
                let key = (p.x().to_bits(), p.y().to_bits());
                if !seen.insert(key) {
                    errors.push(GeometryValidationError::MultiPointDuplicatePoints);
                    break;
                }
            }
        }
        if errors.is_empty() {
            ValidationResult::valid()
        } else {
            ValidationResult::invalid(errors)
        }
    }
}

impl GeoValidation for Line<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        if !self.start.x.is_finite()
            || !self.start.y.is_finite()
            || !self.end.x.is_finite()
            || !self.end.y.is_finite()
        {
            return ValidationResult::invalid(vec![GeometryValidationError::CoordinateNaN]);
        }
        if self.start == self.end {
            return ValidationResult::invalid(vec![GeometryValidationError::ZeroLengthLine(
                self.start,
            )]);
        }
        ValidationResult::valid()
    }
}

impl GeoValidation for LineString<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        let coords = &self.0;
        if coords.len() < 2 {
            return ValidationResult::invalid(vec![GeometryValidationError::RingTooFewPoints {
                found: coords.len(),
                min: 2,
            }]);
        }
        if ring_has_non_finite(coords) {
            return ValidationResult::invalid(vec![GeometryValidationError::CoordinateNaN]);
        }
        for i in 1..coords.len() {
            if coords[i] == coords[i - 1] {
                return ValidationResult::invalid(vec![GeometryValidationError::RepeatedPoint]);
            }
        }
        // OGC Simple Features: LineString must be simple (no self-intersection)
        if check_linestring_self_intersection(coords) {
            return ValidationResult::invalid(vec![GeometryValidationError::NotSimple]);
        }
        ValidationResult::valid()
    }
}

/// Check if a non-closed LineString has self-intersecting segments.
///
/// GEOS isSimple semantics (verified against geosop on the GEOS XML corpus):
/// a LineString is simple iff no two segments intersect except at shared
/// endpoints. This includes proper crossings, a vertex of one segment lying
/// on another segment (vertex-on-edge, including vertex revisits between
/// non-adjacent segments), and collinear overlap beyond a shared point
/// (out-and-back backtracking). Adjacent segments may touch ONLY at their
/// shared vertex; a closed line's first and last segments may touch ONLY at
/// the closure vertex.
pub(crate) fn check_linestring_self_intersection(coords: &[Coord<f64>]) -> bool {
    let n = coords.len() - 1;
    if n < 2 {
        return false;
    }
    let closed = coords[0] == coords[n];
    let scale = {
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for c in coords {
            min_x = min_x.min(c.x);
            max_x = max_x.max(c.x);
            min_y = min_y.min(c.y);
            max_y = max_y.max(c.y);
        }
        (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0)
    };
    let eps = 1e-12 * scale;

    // Adjacent pairs (share a vertex): allowed to touch only at the shared
    // vertex - collinear overlap beyond it (out-and-back) is non-simple.
    for i in 0..n - 1 {
        if segments_collinear_overlap(
            coords[i],
            coords[i + 1],
            coords[i + 1],
            coords[i + 2],
            eps,
        ) {
            return true;
        }
    }
    // Closed line: first and last segments may touch only at the closure vertex.
    if closed
        && segments_collinear_overlap(
            coords[n - 1],
            coords[n],
            coords[0],
            coords[1],
            eps,
        )
    {
        return true;
    }

    // Non-adjacent pairs: any intersection (crossing, vertex-on-edge,
    // vertex revisit, collinear overlap) is non-simple. Uses the ring
    // path's per-pair predicates (edges_intersect_general with a RELATIVE
    // collinear gate + segment-local edges_vertex_on_edge). The global
    // bbox eps (1e-12 * scale) inflates to an absolute length at large
    // coordinate magnitude (measured: 1e15-scale line, eps = 1000 units,
    // flagged a vertex 1 unit from another segment as on-edge: a false
    // NotSimple on the fuzz-discovered mixed-magnitude collapse); the
    // per-pair form keeps the tolerance at the pair's own edge lengths.
    let pair_intersects = |i: usize, j: usize| -> bool {
        if closed && i == 0 && j == n - 1 {
            return false; // closure pair handled above
        }
        // Vertex revisit: non-adjacent segments sharing an endpoint are
        // non-simple ("interior intersection at vertices", GEOS TestSimple
        // LINESTRING (20 80, 80 20, 80 80, 140 60, 80 20, 160 20)).
        // edges_vertex_on_edge excludes endpoint equality (correct for
        // rings, where shared vertices are the normal adjacency), so the
        // line path must flag revisits explicitly.
        if coords[i] == coords[j]
            || coords[i] == coords[j + 1]
            || coords[i + 1] == coords[j]
            || coords[i + 1] == coords[j + 1]
        {
            return true;
        }
        if edges_intersect_general(
            coords[i],
            coords[i + 1],
            coords[j],
            coords[j + 1],
            eps,
        ) {
            return true;
        }
        edges_vertex_on_edge(coords[i], coords[i + 1], coords[j], coords[j + 1])
    };
    if n <= 32 {
        for i in 0..n {
            for j in i + 2..n {
                if pair_intersects(i, j) {
                    return true;
                }
            }
        }
        return false;
    }

    // Vertex revisits, O(n) hash pass (the sweep below cannot see shared
    // vertices - check_edge_pair_intersection excludes endpoint equality,
    // which is correct for rings but not for open chains).
    {
        use rustc_hash::FxHashMap;
        let mut seen: FxHashMap<u64, u32> = FxHashMap::default();
        let key = |c: Coord<f64>| c.x.to_bits() ^ c.y.to_bits().rotate_left(32);
        for (k, c) in coords.iter().enumerate() {
            let kk = k as u32;
            let keyv = key(*c);
            match seen.get(&keyv) {
                Some(&p) => {
                    let pp = p as usize;
                    if !(closed && pp == 0 && k == n) && pp.abs_diff(k) > 1 {
                        return true; // revisit: same vertex, non-adjacent positions
                    }
                }
                None => {
                    seen.insert(keyv, kk);
                }
            }
        }
    }

    // Radix-sort sweep with the ring path's exact predicates. The rstar
    // tree is NOT the primary path: bulk_load costs ~1 us/item (measured
    // 2026-08-07: 948 us to build the tree for a 500-vertex line), the
    // sweep is the proven fast path (50 ms on 600k-vertex giants). Dense
    // x-overlap falls back to the tree (rstar) or the naive pair loop
    // (non-rstar) - same predicates, routing only.
    match crate::validation::sweep::sweep_ring_self_intersects(coords, eps) {
        Some(true) => true,
        Some(false) => false,
        None => {
            #[cfg(feature = "rstar")]
            {
                let tree = build_ls_edge_tree(coords);
                for i in 0..n {
                    let a1 = coords[i];
                    let a2 = coords[i + 1];
                    let (lo_x, hi_x) = if a1.x < a2.x { (a1.x, a2.x) } else { (a2.x, a1.x) };
                    let (lo_y, hi_y) = if a1.y < a2.y { (a1.y, a2.y) } else { (a2.y, a1.y) };
                    let query = rstar::AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]);
                    let found = tree.locate_in_envelope_intersecting_int(query, |c| {
                        let j = c.idx;
                        if j <= i + 1 || (closed && i == 0 && j == n - 1) {
                            return core::ops::ControlFlow::<(), ()>::Continue(());
                        }
                        // Vertex revisit (see the small-path pair check).
                        if a1 == coords[j]
                            || a1 == coords[j + 1]
                            || a2 == coords[j]
                            || a2 == coords[j + 1]
                        {
                            return core::ops::ControlFlow::Break(());
                        }
                        if edges_intersect_general(a1, a2, coords[j], coords[j + 1], eps)
                            || edges_vertex_on_edge(a1, a2, coords[j], coords[j + 1])
                        {
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
            #[cfg(not(feature = "rstar"))]
            {
                for i in 0..n {
                    for j in i + 2..n {
                        if pair_intersects(i, j) {
                            return true;
                        }
                    }
                }
                false
            }
        }
    }
}

/// Full segment-intersection predicate: proper crossing, collinear positive-
/// length overlap, or a vertex of one segment lying on the other.
/// `include_endpoint_share`: for a single line, a non-adjacent segment pair
/// sharing an endpoint is a vertex revisit (non-simple); between MultiLine-
/// String components, endpoint-to-endpoint touching is allowed (OGC Simple
/// Features, verified vs geosop).
fn segments_intersect_any(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
    include_endpoint_share: bool,
) -> bool {
    let o1 = crate::orient::orient2d(a1, a2, b1);
    let o2 = crate::orient::orient2d(a1, a2, b2);
    let o3 = crate::orient::orient2d(b1, b2, a1);
    let o4 = crate::orient::orient2d(b1, b2, a2);

    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    if o1.abs() <= eps && o2.abs() <= eps {
        return segments_collinear_overlap(a1, a2, b1, b2, eps);
    }
    // Vertex-on-segment.
    let on_seg = |p: Coord<f64>, s1: Coord<f64>, s2: Coord<f64>| -> bool {
        if !point_in_segment_bbox(p, s1, s2, eps) {
            return false;
        }
        if include_endpoint_share {
            true
        } else {
            // Cross-component: only STRICT interior contact is non-simple.
            let d1 = (p.x - s1.x).abs().max((p.y - s1.y).abs());
            let d2 = (p.x - s2.x).abs().max((p.y - s2.y).abs());
            d1 > eps && d2 > eps
        }
    };
    if o1.abs() <= eps && on_seg(b1, a1, a2) {
        return true;
    }
    if o2.abs() <= eps && on_seg(b2, a1, a2) {
        return true;
    }
    if o3.abs() <= eps && on_seg(a1, b1, b2) {
        return true;
    }
    if o4.abs() <= eps && on_seg(a2, b1, b2) {
        return true;
    }
    false
}

/// True if both segments are collinear and overlap over a positive-length
/// interval (endpoint-only touching is NOT an overlap).
pub(crate) fn segments_collinear_overlap(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
) -> bool {
    // Fast-FP first: escalate to the exact predicates only when an
    // orientation sits within eps + a relative margin of zero (the fast
    // error is ~4 ulps of L2; the margin covers it, so the shortcut's
    // "not collinear" decision is exactly the exact path's). Measured
    // (2026-08-07): the adjacent-pair loop is the hot cost for valid
    // lines; fast-first cuts it from ~120 ns to ~5 ns per pair.
    let la2 = (a2.x - a1.x).powi(2) + (a2.y - a1.y).powi(2);
    let lb2 = (b2.x - b1.x).powi(2) + (b2.y - b1.y).powi(2);
    let margin = 32.0 * f64::EPSILON * la2.max(lb2);
    let f1 = (a2.x - a1.x) * (b1.y - a1.y) - (a2.y - a1.y) * (b1.x - a1.x);
    let f2 = (a2.x - a1.x) * (b2.y - a1.y) - (a2.y - a1.y) * (b2.x - a1.x);
    if f1.abs() > eps + margin && f2.abs() > eps + margin {
        return false;
    }
    let o1 = crate::orient::orient2d(a1, a2, b1);
    let o2 = crate::orient::orient2d(a1, a2, b2);
    if o1.abs() > eps || o2.abs() > eps {
        return false;
    }
    let dx = a2.x - a1.x;
    let dy = a2.y - a1.y;
    let len2 = dx * dx + dy * dy;
    if len2 <= eps {
        return false;
    }
    let t1 = ((b1.x - a1.x) * dx + (b1.y - a1.y) * dy) / len2;
    let t2 = ((b2.x - a1.x) * dx + (b2.y - a1.y) * dy) / len2;
    let lo = 0.0f64.max(t1.min(t2));
    let hi = 1.0f64.min(t1.max(t2));
    hi - lo > eps
}

/// Bbox containment with epsilon (endpoints included).
fn point_in_segment_bbox(p: Coord<f64>, s1: Coord<f64>, s2: Coord<f64>, eps: f64) -> bool {
    p.x >= s1.x.min(s2.x) - eps
        && p.x <= s1.x.max(s2.x) + eps
        && p.y >= s1.y.min(s2.y) - eps
        && p.y <= s1.y.max(s2.y) + eps
}

/// Check whether two LineString components have any intersecting edges.
pub(crate) fn check_line_components_intersect(
    ls1: &[Coord<f64>],
    ls2: &[Coord<f64>],
    eps: f64,
) -> bool {
    let n1 = ls1.len();
    let n2 = ls2.len();
    if n1 < 2 || n2 < 2 {
        return false;
    }

    // Shared-vertex contact between components. OGC/GEOS rule (verified vs
    // geosop): an intersection point between two components is allowed ONLY
    // when it lies on the BOUNDARY (an endpoint) of BOTH open components.
    // A closed component has an empty boundary, so any shared vertex
    // involving a closed component (or an interior vertex of either) is
    // non-simple: MULTILINESTRING((0 0,1 1),(1 1,2 2)) = simple;
    // MULTILINESTRING((0 0,1 0,1 1,0 0),(0 0,1 1)) = non-simple.
    {
        let open_ends = |ls: &[Coord<f64>]| -> (bool, Coord<f64>, Coord<f64>) {
            let closed = ls.len() > 1 && ls[0] == ls[ls.len() - 1];
            (closed, ls[0], ls[ls.len() - 1])
        };
        let (a_closed, a_first, a_last) = open_ends(ls1);
        let (b_closed, b_first, b_last) = open_ends(ls2);
        let a_boundary = |p: Coord<f64>| !a_closed && (p == a_first || p == a_last);
        let b_boundary = |p: Coord<f64>| !b_closed && (p == b_first || p == b_last);
        // Exact coordinate equality (to_bits) - the corpus uses exact
        // coordinates; near-touches within eps still go through the segment
        // sweep below (vertex-on-interior is always non-simple).
        let mut b_verts: rustc_hash::FxHashSet<(u64, u64)> =
            rustc_hash::FxHashSet::with_capacity_and_hasher(n2, Default::default());
        for c in ls2 {
            b_verts.insert((c.x.to_bits(), c.y.to_bits()));
        }
        for &p in ls1 {
            if !b_verts.contains(&(p.x.to_bits(), p.y.to_bits())) {
                continue;
            }
            if !(a_boundary(p) && b_boundary(p)) {
                return true;
            }
        }
    }

    // Brute force when both components are small
    if n1.max(n2) <= 64 {
        for i in 0..n1 - 1 {
            let a1 = ls1[i];
            let a2 = ls1[i + 1];
            for j in 0..n2 - 1 {
                let b1 = ls2[j];
                let b2 = ls2[j + 1];
                // Cross-component simplicity: interior contact (crossing,
                // vertex-on-interior, collinear overlap) is non-simple;
                // endpoint-to-endpoint touching between components is
                // allowed (OGC Simple Features, verified vs geosop).
                if segments_intersect_any(a1, a2, b1, b2, eps, false) {
                    return true;
                }
            }
        }
        return false;
    }

    #[cfg(feature = "rstar")]
    {
        let (small, large) = if n1 < n2 { (ls1, ls2) } else { (ls2, ls1) };
        let n_small = small.len();
        let tree = build_ls_edge_tree(large);

        for i in 0..n_small - 1 {
            let a1 = small[i];
            let a2 = small[i + 1];
            let (lo_x, hi_x) = if a1.x < a2.x {
                (a1.x, a2.x)
            } else {
                (a2.x, a1.x)
            };
            let (lo_y, hi_y) = if a1.y < a2.y {
                (a1.y, a2.y)
            } else {
                (a2.y, a1.y)
            };
            let query = rstar::AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]);
            let found = tree.locate_in_envelope_intersecting_int(query, |c| {
                let b1 = large[c.idx];
                let b2 = large[c.idx + 1];
                if segments_intersect_any(a1, a2, b1, b2, eps, false) {
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
    #[cfg(not(feature = "rstar"))]
    {
        for i in 0..n1 - 1 {
            let a1 = ls1[i];
            let a2 = ls1[i + 1];
            for j in 0..n2 - 1 {
                let b1 = ls2[j];
                let b2 = ls2[j + 1];
                if edges_intersect_general(a1, a2, b1, b2, eps) {
                    return true;
                }
            }
        }
        false
    }
}

impl GeoValidation for MultiLineString<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        for ls in &self.0 {
            let r = ls.validate();
            if !r.valid {
                errors.extend(r.errors);
            }
        }
        // OGC Simple Features: MultiLineString must not contain duplicate linestrings
        if self.0.len() > 1 {
            let mut seen: rustc_hash::FxHashSet<Vec<(u64, u64)>> =
                rustc_hash::FxHashSet::with_capacity_and_hasher(self.0.len(), Default::default());
            for ls in &self.0 {
                let key: Vec<(u64, u64)> =
                    ls.0.iter()
                        .map(|c| (c.x.to_bits(), c.y.to_bits()))
                        .collect();
                if !seen.insert(key) {
                    errors.push(GeometryValidationError::MultiLineStringDuplicateLines);
                    return ValidationResult::invalid(errors);
                }
            }
        }
        // Cross-component intersection check
        if self.0.len() > 1 {
            // Compute global scale for epsilon
            let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) =
                (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for ls in &self.0 {
                for c in &ls.0 {
                    gmin_x = gmin_x.min(c.x);
                    gmax_x = gmax_x.max(c.x);
                    gmin_y = gmin_y.min(c.y);
                    gmax_y = gmax_y.max(c.y);
                }
            }
            let scale = (gmax_x - gmin_x)
                .abs()
                .max((gmax_y - gmin_y).abs())
                .max(1.0);
            let eps = 1e-12 * scale;

            for i in 0..self.0.len() {
                for j in (i + 1)..self.0.len() {
                    if check_line_components_intersect(&self.0[i].0, &self.0[j].0, eps) {
                        errors.push(GeometryValidationError::NotSimple);
                        return ValidationResult::invalid(errors);
                    }
                }
            }
        }
        if errors.is_empty() {
            ValidationResult::valid()
        } else {
            ValidationResult::invalid(errors)
        }
    }
}

impl GeoValidation for Rect<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        if !self.min().x.is_finite()
            || !self.min().y.is_finite()
            || !self.max().x.is_finite()
            || !self.max().y.is_finite()
        {
            return ValidationResult::invalid(vec![GeometryValidationError::CoordinateNaN]);
        }
        if (self.max().x - self.min().x).abs() < f64::EPSILON
            || (self.max().y - self.min().y).abs() < f64::EPSILON
        {
            return ValidationResult::invalid(vec![GeometryValidationError::DegenerateExterior]);
        }
        ValidationResult::valid()
    }
}

impl GeoValidation for Triangle<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        let coords = [self.v1(), self.v2(), self.v3()];
        if ring_has_non_finite(&coords) {
            return ValidationResult::invalid(vec![GeometryValidationError::CoordinateNaN]);
        }
        if coords[0] == coords[1] || coords[1] == coords[2] || coords[0] == coords[2] {
            return ValidationResult::invalid(vec![GeometryValidationError::DegenerateExterior]);
        }
        // Zero or near-zero area (collinear)
        let area = ((coords[1].x - coords[0].x) * (coords[2].y - coords[0].y)
            - (coords[1].y - coords[0].y) * (coords[2].x - coords[0].x))
            .abs();
        if area < 1e-12 {
            return ValidationResult::invalid(vec![GeometryValidationError::CollinearRing]);
        }
        ValidationResult::valid()
    }
}

// ---------------------------------------------------------------------------
// Free functions - convenience wrappers around GeoValidation
// ---------------------------------------------------------------------------

/// Check whether a geometry is OGC-valid.
///
/// Convenience wrapper around [`GeoValidation::is_valid`] that does not
/// require importing the trait.
pub fn is_valid(geom: &geo::Geometry<f64>) -> bool {
    GeoValidation::is_valid(geom)
}

/// Validate a geometry, returning all OGC violations found.
///
/// Convenience wrapper around [`GeoValidation::validate`].
pub fn validate(geom: &geo::Geometry<f64>) -> ValidationResult {
    GeoValidation::validate(geom)
}

/// Validate and return a human-readable description of violations.
///
/// Returns `"Valid Geometry"` when the geometry passes all checks.
///
/// Convenience wrapper around [`GeoValidation::validate_reason`].
pub fn validate_reason(geom: &geo::Geometry<f64>) -> String {
    GeoValidation::validate_reason(geom)
}

use geo::{
    Coord, GeoFloat, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};
use rstar::{RTree, RTreeObject, AABB};
use thiserror::Error;

#[derive(Error, Clone, Debug, PartialEq)]
pub enum GeometryValidationError {
    #[error("Coordinate is NaN")]
    CoordinateNaN,

    #[error("Ring has too few points: found {found}, minimum required {min}")]
    RingTooFewPoints { found: usize, min: usize },

    #[error("Ring is not closed: first {first:?} != last {last:?}")]
    RingNotClosed { first: Coord<f64>, last: Coord<f64> },

    #[error("Ring has self-intersections")]
    SelfIntersection,

    #[error("Ring has repeated non-consecutive vertices (pinch point)")]
    PinchPoint,

    #[error("Hole lies outside shell")]
    HoleOutsideShell,

    #[error("Holes are nested")]
    NestedHoles,

    #[error("Interior ring is disconnected from shell")]
    DisconnectedInteriorRing,

    #[error("Wrong ring orientation: exterior should be CCW, interior CW")]
    WrongOrientation,

    #[error("Collinear ring: all points lie on a line")]
    CollinearRing,

    #[error("Geometry has repeated (duplicate) points")]
    RepeatedPoint,

    #[error("Geometry contains duplicate rings")]
    DuplicatedRings,

    #[error("MultiPoint contains duplicate points")]
    MultiPointDuplicatePoints,

    #[error("MultiLineString contains duplicate linestrings")]
    MultiLineStringDuplicateLines,

    #[error("Line has zero length (start == end at {0:?})")]
    ZeroLengthLine(Coord<f64>),

    #[error("Polygon exterior ring is degenerate (collapsed)")]
    DegenerateExterior,

    #[error("Geometry is not simple: components intersect at interior points")]
    NotSimple,

    #[error("GeometryCollection nesting exceeds maximum depth")]
    ExcessiveNesting,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<GeometryValidationError>,
}

impl ValidationResult {
    pub fn valid() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
        }
    }

    pub fn invalid(errors: Vec<GeometryValidationError>) -> Self {
        Self {
            valid: false,
            errors,
        }
    }
}

pub trait GeoValidation {
    type Scalar: GeoFloat;

    fn is_valid(&self) -> bool {
        self.validate().valid
    }

    fn validate(&self) -> ValidationResult;

    fn validate_reason(&self) -> String {
        let result = self.validate();
        if result.valid {
            "Valid Geometry".to_string()
        } else {
            result
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        }
    }
}

fn ring_has_non_finite(ring: &[Coord<f64>]) -> bool {
    ring.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
}

fn check_ring_validity(ring: &[Coord<f64>], is_exterior: bool) -> Vec<GeometryValidationError> {
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

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for c in &ring[..n] {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * scale;
    if (max_x - min_x).abs() < f64::EPSILON * scale || (max_y - min_y).abs() < f64::EPSILON * scale
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
        let key = (c.x.to_bits(), c.y.to_bits());
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

    struct EdgeEnv {
        idx: u32,
        env: AABB<[f64; 2]>,
    }
    impl RTreeObject for EdgeEnv {
        type Envelope = AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }
    if n > 64 {
        let mut edges = Vec::with_capacity(n);
        for i in 0..n {
            let (lo_x, hi_x) = if ring[i].x < ring[(i + 1) % n].x {
                (ring[i].x, ring[(i + 1) % n].x)
            } else {
                (ring[(i + 1) % n].x, ring[i].x)
            };
            let (lo_y, hi_y) = if ring[i].y < ring[(i + 1) % n].y {
                (ring[i].y, ring[(i + 1) % n].y)
            } else {
                (ring[(i + 1) % n].y, ring[i].y)
            };
            let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
            edges.push(EdgeEnv {
                idx: i as u32,
                env: AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext]),
            });
        }
        let tree = RTree::bulk_load(edges);
        for i in 0..n {
            let (lo_x, hi_x) = if ring[i].x < ring[(i + 1) % n].x {
                (ring[i].x, ring[(i + 1) % n].x)
            } else {
                (ring[(i + 1) % n].x, ring[i].x)
            };
            let (lo_y, hi_y) = if ring[i].y < ring[(i + 1) % n].y {
                (ring[i].y, ring[(i + 1) % n].y)
            } else {
                (ring[(i + 1) % n].y, ring[i].y)
            };
            let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
            let env = AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext]);
            let found = tree.locate_in_envelope_intersecting_int(&env, |c| {
                let j = c.idx as usize;
                if j <= i {
                    return std::ops::ControlFlow::Continue(());
                }
                if i.abs_diff(j) <= 1 || (i == 0 && j == n - 1) {
                    return std::ops::ControlFlow::Continue(());
                }
                if check_edge_pair_intersection(ring, i, j, eps) {
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
                }
            });
            if found.is_break() {
                errors.push(GeometryValidationError::SelfIntersection);
                return errors;
            }
        }
    } else {
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

    errors
}

fn edges_intersect_general(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
) -> bool {
    let o1 = (a2.x - a1.x) * (b1.y - a1.y) - (a2.y - a1.y) * (b1.x - a1.x);
    let o2 = (a2.x - a1.x) * (b2.y - a1.y) - (a2.y - a1.y) * (b2.x - a1.x);
    let o3 = (b2.x - b1.x) * (a1.y - b1.y) - (b2.y - b1.y) * (a1.x - b1.x);
    let o4 = (b2.x - b1.x) * (a2.y - b1.y) - (b2.y - b1.y) * (a2.x - b1.x);

    // Proper crossing
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }

    // Collinear overlap (excluding endpoint-only touching)
    let collinear = o1.abs() < eps && o2.abs() < eps;
    if collinear {
        let dx = a2.x - a1.x;
        let dy = a2.y - a1.y;
        let len2 = dx * dx + dy * dy;
        if len2 > eps {
            let t1 = ((b1.x - a1.x) * dx + (b1.y - a1.y) * dy) / len2;
            let t2 = ((b2.x - a1.x) * dx + (b2.y - a1.y) * dy) / len2;
            let lo = 0.0f64.max(t1.min(t2));
            let hi = 1.0f64.min(t1.max(t2));
            if hi - lo > eps {
                return true;
            }
        }
    }

    false
}

fn check_edge_pair_intersection(coords: &[Coord<f64>], i: usize, j: usize, eps: f64) -> bool {
    let n = coords.len() - 1;
    let a1 = coords[i];
    let a2 = coords[(i + 1) % n];
    let b1 = coords[j];
    let b2 = coords[(j + 1) % n];
    edges_intersect_general(a1, a2, b1, b2, eps)
}

/// Check whether two rings (from different polygons) have any intersecting edges.
/// Touching at a single vertex is allowed (OGC), but crossing, overlapping, or
/// touching along an edge is not.
fn check_rings_intersect(ring1: &[Coord<f64>], ring2: &[Coord<f64>], eps: f64) -> bool {
    let n1 = ring1.len() - 1;
    let n2 = ring2.len() - 1;
    if n1 < 2 || n2 < 2 {
        return false;
    }
    for i in 0..n1 {
        let a1 = ring1[i];
        let a2 = ring1[(i + 1) % n1];
        for j in 0..n2 {
            let b1 = ring2[j];
            let b2 = ring2[(j + 1) % n2];
            if edges_intersect_general(a1, a2, b1, b2, eps) {
                return true;
            }
        }
    }
    false
}

fn check_orientation(ring: &[Coord<f64>]) -> bool {
    let n = ring.len() - 1;
    if n < 3 {
        return true;
    }
    let mut signed_area = 0.0;
    for i in 0..n {
        let (x1, y1) = (ring[i].x, ring[i].y);
        let (x2, y2) = (ring[(i + 1) % n].x, ring[(i + 1) % n].y);
        signed_area += x1 * y2 - x2 * y1;
    }
    signed_area > 0.0
}

fn point_in_ring_exclusive(pt: Coord<f64>, ring: &[Coord<f64>]) -> bool {
    let n = ring.len();
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

fn point_on_segment(pt: Coord<f64>, a: Coord<f64>, b: Coord<f64>, eps: f64) -> bool {
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

fn point_on_ring(pt: Coord<f64>, ring: &[Coord<f64>], eps: f64) -> bool {
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

/// Check whether two rings (with closing point) are duplicates starting at a
/// different vertex. Both rings must have the same length and contain the same
/// sequence of coordinates up to a cyclic rotation.
fn is_rotated_duplicate(a: &[Coord<f64>], b: &[Coord<f64>]) -> bool {
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

fn check_holes_valid(
    shell: &[Coord<f64>],
    interiors: &[LineString<f64>],
) -> Vec<GeometryValidationError> {
    let mut errors = Vec::new();

    // Compute scale-relative epsilon for boundary checks
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for c in shell {
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }
    let scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * scale;

    for hole in interiors {
        // Check if hole edges cross the shell boundary (hole not fully inside)
        if check_rings_intersect(&hole.0[..], shell, eps) {
            errors.push(GeometryValidationError::HoleOutsideShell);
            return errors;
        }

        // A hole touching the shell at ≥ 2 distinct points may disconnect the interior
        let touch_count = hole
            .0
            .iter()
            .filter(|&&hp| point_on_ring(hp, shell, eps))
            .count();
        if touch_count >= 2 {
            errors.push(GeometryValidationError::DisconnectedInteriorRing);
            return errors;
        }

        // If no hole vertex is strictly inside the shell, the hole is entirely outside.
        // Single-point tangent touches (touch_count == 1) are valid per OGC.
        let any_inside = hole.0.iter().any(|&hp| point_in_ring_exclusive(hp, shell));
        if !any_inside {
            errors.push(GeometryValidationError::HoleOutsideShell);
            return errors;
        }
    }
    let holes: Vec<&[Coord<f64>]> = interiors.iter().map(|h| &h.0[..]).collect();
    if holes.len() > 1 {
        // --- hole-hole edge intersection check (disconnected interior) ---
        for i in 0..holes.len() {
            for j in (i + 1)..holes.len() {
                if check_rings_intersect(holes[i], holes[j], eps) {
                    errors.push(GeometryValidationError::DisconnectedInteriorRing);
                    return errors;
                }
            }
        }

        // --- nesting check ---
        struct HoleEnv2 {
            idx: usize,
            env: AABB<[f64; 2]>,
        }
        impl RTreeObject for HoleEnv2 {
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
            envs.push(HoleEnv2 {
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
            let _ = tree.locate_in_envelope_intersecting_int(&query, |c| {
                if c.idx != i && point_in_ring_exclusive(pt, holes[c.idx]) {
                    overlaps = true;
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
                }
            });
            if overlaps {
                errors.push(GeometryValidationError::NestedHoles);
                return errors;
            }
        }
    }
    errors
}

impl GeoValidation for Point<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        if !self.0.x.is_finite() || !self.0.y.is_finite() {
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
        ValidationResult::valid()
    }
}

/// Check whether two LineString components have any intersecting edges.
fn check_line_components_intersect(ls1: &[Coord<f64>], ls2: &[Coord<f64>], eps: f64) -> bool {
    let n1 = ls1.len();
    let n2 = ls2.len();
    if n1 < 2 || n2 < 2 {
        return false;
    }
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
            for i in 0..self.0.len() {
                for j in (i + 1)..self.0.len() {
                    if self.0[i].0 == self.0[j].0 {
                        errors.push(GeometryValidationError::MultiLineStringDuplicateLines);
                        // Report only once
                        if !errors.is_empty() {
                            return ValidationResult::invalid(errors);
                        }
                    }
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

impl GeoValidation for Polygon<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();

        let ext_errors = check_ring_validity(&self.exterior().0, true);
        if !ext_errors.is_empty() {
            errors.extend(ext_errors);
            return ValidationResult::invalid(errors);
        }

        if !check_orientation(&self.exterior().0) {
            errors.push(GeometryValidationError::WrongOrientation);
        }

        if self.interiors().is_empty() {
            if errors.is_empty() {
                return ValidationResult::valid();
            }
            return ValidationResult::invalid(errors);
        }

        let interiors: Vec<&[Coord<f64>]> = self.interiors().iter().map(|h| &h.0[..]).collect();

        // Check for duplicate rings (including rotated-start duplicates)
        for (i, h1) in interiors.iter().enumerate() {
            for h2 in interiors.iter().skip(i + 1) {
                if is_rotated_duplicate(h1, h2) {
                    errors.push(GeometryValidationError::DuplicatedRings);
                    return ValidationResult::invalid(errors);
                }
            }
            if is_rotated_duplicate(h1, &self.exterior().0) {
                errors.push(GeometryValidationError::DuplicatedRings);
                return ValidationResult::invalid(errors);
            }
        }

        for hole in self.interiors() {
            let hole_errors = check_ring_validity(&hole.0, false);
            if !hole_errors.is_empty() {
                errors.extend(hole_errors);
                continue;
            }
            if check_orientation(&hole.0) {
                errors.push(GeometryValidationError::WrongOrientation);
            }
        }

        let hole_containment_errors = check_holes_valid(&self.exterior().0, self.interiors());
        errors.extend(hole_containment_errors);

        if errors.is_empty() {
            ValidationResult::valid()
        } else {
            ValidationResult::invalid(errors)
        }
    }
}

impl GeoValidation for MultiPolygon<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        for p in &self.0 {
            let r = p.validate();
            if !r.valid {
                errors.extend(r.errors);
            }
        }

        let shells: Vec<&[Coord<f64>]> = self.0.iter().map(|p| &p.exterior().0[..]).collect();

        // Check for duplicate shells (including rotated-start duplicates)
        for i in 0..shells.len() {
            for j in (i + 1)..shells.len() {
                if is_rotated_duplicate(shells[i], shells[j]) {
                    errors.push(GeometryValidationError::DuplicatedRings);
                    return ValidationResult::invalid(errors);
                }
            }
        }

        if shells.len() > 1 {
            // Compute global scale for intersection epsilon
            let (mut gmin_x, mut gmax_x, mut gmin_y, mut gmax_y) =
                (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for s in &shells {
                for c in *s {
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

            // Cross-ring edge-edge intersection check (must run before nesting
            // check — partial overlaps can produce false-positive nesting)
            for i in 0..shells.len() {
                for j in (i + 1)..shells.len() {
                    if check_rings_intersect(shells[i], shells[j], eps) {
                        errors.push(GeometryValidationError::SelfIntersection);
                        return ValidationResult::invalid(errors);
                    }
                }
            }

            // Nesting check: one shell fully inside another
            struct ShellEnv {
                idx: usize,
                env: AABB<[f64; 2]>,
            }
            impl RTreeObject for ShellEnv {
                type Envelope = AABB<[f64; 2]>;
                fn envelope(&self) -> Self::Envelope {
                    self.env
                }
            }
            let mut envs = Vec::with_capacity(shells.len());
            for (i, s) in shells.iter().enumerate() {
                let first = s.first().map(|c| (c.x, c.y)).unwrap_or((0.0, 0.0));
                let (mut min_x, mut max_x, mut min_y, mut max_y) =
                    (first.0, first.0, first.1, first.1);
                for c in *s {
                    min_x = min_x.min(c.x);
                    max_x = max_x.max(c.x);
                    min_y = min_y.min(c.y);
                    max_y = max_y.max(c.y);
                }
                envs.push(ShellEnv {
                    idx: i,
                    env: AABB::from_corners([min_x, min_y], [max_x, max_y]),
                });
            }
            let tree = RTree::bulk_load(envs);
            for (i, s2) in shells.iter().enumerate() {
                let Some(pt) = s2.first().copied() else {
                    continue;
                };
                let query = AABB::from_corners([pt.x, pt.y], [pt.x, pt.y]);
                let mut overlaps = false;
                let _ = tree.locate_in_envelope_intersecting_int(&query, |c| {
                    if c.idx != i && point_in_ring_exclusive(pt, shells[c.idx]) {
                        overlaps = true;
                        std::ops::ControlFlow::Break(())
                    } else {
                        std::ops::ControlFlow::<(), ()>::Continue(())
                    }
                });
                if overlaps {
                    errors.push(GeometryValidationError::NestedHoles);
                    return ValidationResult::invalid(errors);
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

trait ValidateDepth {
    fn validate_at_depth(&self, depth: usize, max_depth: usize) -> ValidationResult;
}

impl ValidateDepth for Geometry<f64> {
    fn validate_at_depth(&self, depth: usize, max_depth: usize) -> ValidationResult {
        match self {
            Geometry::GeometryCollection(gc) => gc.validate_at_depth(depth, max_depth),
            _ => self.validate(),
        }
    }
}

impl ValidateDepth for GeometryCollection<f64> {
    fn validate_at_depth(&self, depth: usize, max_depth: usize) -> ValidationResult {
        if depth > max_depth {
            return ValidationResult::invalid(vec![GeometryValidationError::ExcessiveNesting]);
        }
        let mut errors = Vec::new();
        for g in &self.0 {
            let r = g.validate_at_depth(depth + 1, max_depth);
            if !r.valid {
                errors.extend(r.errors);
            }
        }
        if errors.is_empty() {
            ValidationResult::valid()
        } else {
            ValidationResult::invalid(errors)
        }
    }
}

impl GeoValidation for Geometry<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        match self {
            Geometry::Point(g) => g.validate(),
            Geometry::Line(g) => g.validate(),
            Geometry::LineString(g) => g.validate(),
            Geometry::Polygon(g) => g.validate(),
            Geometry::MultiPoint(g) => g.validate(),
            Geometry::MultiLineString(g) => g.validate(),
            Geometry::MultiPolygon(g) => g.validate(),
            Geometry::GeometryCollection(g) => g.validate(),
            Geometry::Rect(g) => g.validate(),
            Geometry::Triangle(g) => g.validate(),
        }
    }
}

impl GeoValidation for GeometryCollection<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        self.validate_at_depth(0, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_valid() {
        assert!(Point::new(1.0, 2.0).is_valid());
    }

    #[test]
    fn test_point_nan() {
        assert!(!Point::new(f64::NAN, 2.0).is_valid());
    }

    #[test]
    fn test_line_valid() {
        let l = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
        assert!(l.is_valid());
    }

    #[test]
    fn test_line_degenerate() {
        let l = Line::new(Coord { x: 1.0, y: 1.0 }, Coord { x: 1.0, y: 1.0 });
        assert!(!l.is_valid());
        assert!(l.validate_reason().contains("zero length"));
    }

    #[test]
    fn test_linestring_valid() {
        let ls = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        ]);
        assert!(ls.is_valid());
    }

    #[test]
    fn test_linestring_empty() {
        let ls = LineString::<f64>::new(Vec::new());
        assert!(!ls.is_valid());
    }

    #[test]
    fn test_polygon_valid() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(poly.is_valid());
    }

    #[test]
    fn test_ring_not_closed() {
        let ring = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
        ]);
        let result = check_ring_validity(&ring.0, true);
        assert!(!result.is_empty());
        assert!(result
            .iter()
            .any(|e| matches!(e, GeometryValidationError::RingNotClosed { .. })));
    }

    #[test]
    fn test_polygon_too_few_points() {
        let poly = Polygon::new(
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
            Vec::new(),
        );
        assert!(!poly.is_valid());
    }

    #[test]
    fn test_bowtie_self_intersects() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(!poly.is_valid());
        assert_eq!(
            poly.validate().errors[0],
            GeometryValidationError::SelfIntersection
        );
    }

    #[test]
    fn test_polygon_with_hole() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 20.0, y: 0.0 },
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 0.0, y: 20.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 5.0, y: 15.0 },
                Coord { x: 15.0, y: 15.0 },
                Coord { x: 15.0, y: 5.0 },
            ])],
        );
        assert!(poly.is_valid());
    }

    #[test]
    fn test_hole_outside_shell() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 25.0, y: 20.0 },
                Coord { x: 25.0, y: 25.0 },
                Coord { x: 20.0, y: 25.0 },
                Coord { x: 20.0, y: 20.0 },
            ])],
        );
        assert!(!poly.is_valid());
    }

    #[test]
    fn test_validate_reason_valid() {
        let p = Point::new(1.0, 2.0);
        assert_eq!(p.validate_reason(), "Valid Geometry");
    }

    #[test]
    fn test_multipolygon_overlapping() {
        let mp = MultiPolygon::new(vec![
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 0.0 },
                    Coord { x: 5.0, y: 0.0 },
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 0.0, y: 5.0 },
                    Coord { x: 0.0, y: 0.0 },
                ]),
                Vec::new(),
            ),
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 2.0, y: 2.0 },
                    Coord { x: 7.0, y: 2.0 },
                    Coord { x: 7.0, y: 7.0 },
                    Coord { x: 2.0, y: 7.0 },
                    Coord { x: 2.0, y: 2.0 },
                ]),
                Vec::new(),
            ),
        ]);
        assert!(!mp.is_valid());
    }

    #[test]
    fn test_multipolygon_shells_cross() {
        // Two shells that cross (neither contains the other's first point)
        let mp = MultiPolygon::new(vec![
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 0.0, y: 3.0 },
                    Coord { x: 10.0, y: 3.0 },
                    Coord { x: 10.0, y: 5.0 },
                    Coord { x: 0.0, y: 5.0 },
                    Coord { x: 0.0, y: 3.0 },
                ]),
                Vec::new(),
            ),
            Polygon::new(
                LineString::new(vec![
                    Coord { x: 4.0, y: 0.0 },
                    Coord { x: 6.0, y: 0.0 },
                    Coord { x: 6.0, y: 8.0 },
                    Coord { x: 4.0, y: 8.0 },
                    Coord { x: 4.0, y: 0.0 },
                ]),
                Vec::new(),
            ),
        ]);
        assert!(!mp.is_valid());
    }

    #[test]
    fn test_triangle_valid() {
        let t = Triangle::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 5.0, y: 0.0 },
            Coord { x: 2.5, y: 5.0 },
        );
        assert!(t.is_valid());
    }

    #[test]
    fn test_triangle_degenerate() {
        let t = Triangle::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 2.0, y: 2.0 },
        );
        assert!(!t.is_valid());
    }

    #[test]
    fn test_rect_valid() {
        let r = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert!(r.is_valid());
    }

    #[test]
    fn test_rect_nan() {
        let r = Rect::new(Point::new(f64::NAN, 0.0), Point::new(10.0, 10.0));
        assert!(!r.is_valid());
    }

    #[test]
    fn test_geometry_dispatch() {
        let g = Geometry::Point(Point::new(1.0, 2.0));
        assert!(g.is_valid());

        let g2 = Geometry::Point(Point::new(f64::NAN, 2.0));
        assert!(!g2.is_valid());
    }

    #[test]
    fn test_geometry_collection() {
        let gc = GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::Point(Point::new(f64::NAN, 2.0)),
        ]);
        assert!(!gc.is_valid());
        assert_eq!(gc.validate().errors.len(), 1);
    }

    #[test]
    fn test_multilinestring_not_simple() {
        let mls = MultiLineString::new(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 }]),
            LineString::new(vec![Coord { x: 0.0, y: 10.0 }, Coord { x: 10.0, y: 0.0 }]),
        ]);
        let result = mls.validate();
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::NotSimple)));
    }

    #[test]
    fn test_multilinestring_simple() {
        let mls = MultiLineString::new(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 0.0 }]),
            LineString::new(vec![Coord { x: 0.0, y: 10.0 }, Coord { x: 10.0, y: 10.0 }]),
        ]);
        assert!(mls.is_valid());
    }

    #[test]
    fn test_linestring_zero_length() {
        let ls = LineString::new(vec![Coord { x: 1.0, y: 2.0 }, Coord { x: 1.0, y: 2.0 }]);
        assert!(!ls.is_valid());
    }

    #[test]
    fn test_linestring_zero_length_many_coords() {
        let ls = LineString::new(vec![
            Coord { x: 3.0, y: 4.0 },
            Coord { x: 3.0, y: 4.0 },
            Coord { x: 3.0, y: 4.0 },
            Coord { x: 3.0, y: 4.0 },
        ]);
        assert!(!ls.is_valid());
    }

    #[test]
    fn test_hole_edges_cross_shell() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![LineString::new(vec![
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 12.0, y: 5.0 },
                Coord { x: 12.0, y: 8.0 },
                Coord { x: 5.0, y: 8.0 },
                Coord { x: 5.0, y: 5.0 },
            ])],
        );
        assert!(!poly.is_valid());
    }

    #[test]
    fn test_excessive_nesting() {
        let inner = Geometry::Point(Point::new(1.0, 2.0));
        let mut gc = GeometryCollection(vec![inner]);
        for _ in 0..150 {
            gc = GeometryCollection(vec![Geometry::GeometryCollection(gc)]);
        }
        assert!(!gc.is_valid());
        assert!(gc
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::ExcessiveNesting)));
    }
}

use geo::{Coord, GeoFloat, Line, LineString, MultiLineString, MultiPoint, Point, Rect, Triangle};
use thiserror::Error;

/// Errors reported by OGC geometry validation.
///
/// Each variant corresponds to an OGC Simple Features validity rule.
#[derive(Error, Clone, Debug, PartialEq)]
pub enum GeometryValidationError {
    /// One or more coordinates contain NaN or infinite values.
    #[error("Coordinate is NaN")]
    CoordinateNaN,

    /// A ring does not have enough distinct vertices (min 4 for rings).
    #[error("Ring has too few points: found {found}, minimum required {min}")]
    RingTooFewPoints { found: usize, min: usize },

    /// A ring's first and last coordinates are not equal (not closed).
    #[error("Ring is not closed: first {first:?} != last {last:?}")]
    RingNotClosed { first: Coord<f64>, last: Coord<f64> },

    /// A ring has edges that cross or overlap non-adjacent edges.
    #[error("Ring has self-intersections")]
    SelfIntersection,

    /// A ring has a non-consecutive repeated vertex (pinch point).
    #[error("Ring has repeated non-consecutive vertices (pinch point)")]
    PinchPoint,

    /// A polygon hole lies partially or fully outside its shell.
    #[error("Hole lies outside shell")]
    HoleOutsideShell,

    /// Two or more polygon holes are nested inside each other.
    #[error("Holes are nested")]
    NestedHoles,

    /// An interior ring is disconnected from the shell (touching at ≥ 2 points or edges crossing).
    #[error("Interior ring is disconnected from shell")]
    DisconnectedInteriorRing,

    /// Ring winding direction is incorrect (exterior must be CCW, interior CW).
    #[error("Wrong ring orientation: exterior should be CCW, interior CW")]
    WrongOrientation,

    /// All vertices of a ring are collinear (zero area).
    #[error("Collinear ring: all points lie on a line")]
    CollinearRing,

    /// Consecutive duplicate coordinates found in a geometry.
    #[error("Geometry has repeated (duplicate) points")]
    RepeatedPoint,

    /// A polygon contains two or more identical rings.
    #[error("Geometry contains duplicate rings")]
    DuplicatedRings,

    /// A MultiPoint contains the same point more than once.
    #[error("MultiPoint contains duplicate points")]
    MultiPointDuplicatePoints,

    /// A MultiLineString contains the same linestring more than once.
    #[error("MultiLineString contains duplicate linestrings")]
    MultiLineStringDuplicateLines,

    /// A Line has zero length (start and end coordinates are equal).
    #[error("Line has zero length (start == end at {0:?})")]
    ZeroLengthLine(Coord<f64>),

    /// A polygon's exterior ring has degenerated to a line or point.
    #[error("Polygon exterior ring is degenerate (collapsed)")]
    DegenerateExterior,

    /// A LineString or MultiLineString has components that intersect at interior points.
    #[error("Geometry is not simple: components intersect at interior points")]
    NotSimple,

    /// A GeometryCollection has exceeded the maximum nesting depth.
    #[error("GeometryCollection nesting exceeds maximum depth")]
    ExcessiveNesting,
}

/// Result of an OGC validity check.
///
/// Contains the overall valid/invalid status and a list of detailed
/// [`GeometryValidationError`] entries describing each violation found.
///
/// # Examples
///
/// ```rust
/// # use geo::{Geometry, Point};
/// # let geometry = Geometry::Point(Point::new(0.0, 0.0));
/// use geo_repair::{validate, ValidationResult};
///
/// let result = validate(&geometry);
/// if result.valid {
///     println!("Geometry is valid");
/// } else {
///     for err in &result.errors {
///         println!("  Violation: {err}");
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationResult {
    /// Whether the geometry passed all OGC validity checks.
    pub valid: bool,
    /// List of validity violations found. Empty when `valid` is true.
    pub errors: Vec<GeometryValidationError>,
}

impl ValidationResult {
    /// Create a result indicating a valid geometry (no errors).
    pub fn valid() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
        }
    }

    /// Create a result indicating an invalid geometry with the given errors.
    pub fn invalid(errors: Vec<GeometryValidationError>) -> Self {
        Self {
            valid: false,
            errors,
        }
    }

    /// Human-readable validity reason (like GEOS `isValidReason`).
    ///
    /// Returns `"Valid Geometry"` when valid, or a semicolon-separated list of
    /// violations when invalid.
    pub fn reason(&self) -> String {
        if self.valid {
            "Valid Geometry".to_string()
        } else {
            self.errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        }
    }
}

/// Trait for OGC geometry validation.
///
/// Implemented for all geometry types. Call [`validate`](GeoValidation::validate)
/// to get a [`ValidationResult`] with all violations, or
/// [`is_valid`](GeoValidation::is_valid) for a quick boolean check.
pub trait GeoValidation {
    /// The scalar coordinate type (e.g. `f64`, `f32`).
    type Scalar: GeoFloat;

    /// Quick validity check — returns `true` if the geometry passes all OGC rules.
    fn is_valid(&self) -> bool {
        self.validate().valid
    }

    /// Full validation — returns a [`ValidationResult`] with all violations found.
    fn validate(&self) -> ValidationResult;

    /// Human-readable validity reason (like GEOS `isValidReason`).
    ///
    /// Returns `"Valid Geometry"` when valid, or a semicolon-separated list
    /// of violation descriptions when invalid.
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

pub(crate) fn ring_has_non_finite(ring: &[Coord<f64>]) -> bool {
    ring.iter().any(|c| !c.x.is_finite() || !c.y.is_finite())
}

pub(crate) fn check_ring_validity(
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

    #[cfg(feature = "rstar")]
    {
        struct EdgeEnv {
            idx: u32,
            env: rstar::AABB<[f64; 2]>,
        }
        impl rstar::RTreeObject for EdgeEnv {
            type Envelope = rstar::AABB<[f64; 2]>;
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
                    env: rstar::AABB::from_corners(
                        [lo_x - ext, lo_y - ext],
                        [hi_x + ext, hi_y + ext],
                    ),
                });
            }
            let tree = rstar::RTree::bulk_load(edges);
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
                let env =
                    rstar::AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext]);
                let found = tree.locate_in_envelope_intersecting_int(env, |c| {
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

    errors
}

pub(crate) fn edges_intersect_general(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
) -> bool {
    // Fast f64 orient2d for validation. The repair pipeline uses Shewchuk
    // for precision-critical intersection computation.
    let o1 = crate::orient::orient2d_fast(a1, a2, b1);
    let o2 = crate::orient::orient2d_fast(a1, a2, b2);
    let o3 = crate::orient::orient2d_fast(b1, b2, a1);
    let o4 = crate::orient::orient2d_fast(b1, b2, a2);

    // Proper crossing
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }

    // Collinear overlap (excluding endpoint-only touching)
    let collinear = o1.abs() <= eps && o2.abs() <= eps;
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

pub(crate) fn check_edge_pair_intersection(
    coords: &[Coord<f64>],
    i: usize,
    j: usize,
    eps: f64,
) -> bool {
    let n = coords.len() - 1;
    let a1 = coords[i];
    let a2 = coords[(i + 1) % n];
    let b1 = coords[j];
    let b2 = coords[(j + 1) % n];
    edges_intersect_general(a1, a2, b1, b2, eps)
}

/// Minimal edge-index wrapper for R-tree intersection queries.
#[cfg(feature = "rstar")]
struct EdgeIdx {
    idx: usize,
    env: rstar::AABB<[f64; 2]>,
}
#[cfg(feature = "rstar")]
impl rstar::RTreeObject for EdgeIdx {
    type Envelope = rstar::AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

/// Build an R-tree over a ring's edges (wrapping at len-1 for closing point).
#[cfg(feature = "rstar")]
fn build_ring_edge_tree(ring: &[Coord<f64>]) -> rstar::RTree<EdgeIdx> {
    let n = ring.len() - 1;
    rstar::RTree::bulk_load(
        (0..n)
            .map(|i| {
                let a = ring[i];
                let b = ring[(i + 1) % n];
                let (lo_x, hi_x) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
                let (lo_y, hi_y) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
                EdgeIdx {
                    idx: i,
                    env: rstar::AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]),
                }
            })
            .collect(),
    )
}

/// Build an R-tree over a linestring's segments (non-ring, no wrap-around).
#[cfg(feature = "rstar")]
fn build_ls_edge_tree(coords: &[Coord<f64>]) -> rstar::RTree<EdgeIdx> {
    let n = coords.len() - 1;
    if n < 1 {
        return rstar::RTree::bulk_load(Vec::new());
    }
    rstar::RTree::bulk_load(
        (0..n)
            .map(|i| {
                let a = coords[i];
                let b = coords[i + 1];
                let (lo_x, hi_x) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
                let (lo_y, hi_y) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
                EdgeIdx {
                    idx: i,
                    env: rstar::AABB::from_corners([lo_x, lo_y], [hi_x, hi_y]),
                }
            })
            .collect(),
    )
}

/// Check whether two rings (from different polygons) have any intersecting edges.
/// Touching at a single vertex is allowed (OGC), but crossing, overlapping, or
/// touching along an edge is not.
pub(crate) fn check_rings_intersect(ring1: &[Coord<f64>], ring2: &[Coord<f64>], eps: f64) -> bool {
    let n1 = ring1.len().max(2) - 1;
    let n2 = ring2.len().max(2) - 1;
    if n1 < 2 || n2 < 2 {
        return false;
    }

    // Brute-force when both rings are small — faster than building a tree.
    if n1.max(n2) <= 64 {
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
        return false;
    }

    // Large rings: build tree over the smaller ring, query each edge of the
    // larger ring via envelope intersection.
    #[cfg(feature = "rstar")]
    {
        let (build_ring, query_ring, n_query) = if n1 < n2 {
            (ring1, ring2, n2)
        } else {
            (ring2, ring1, n1)
        };
        let n_build = build_ring.len() - 1;
        let tree = build_ring_edge_tree(build_ring);

        for i in 0..n_query {
            let a1 = query_ring[i];
            let a2 = query_ring[(i + 1) % n_query];
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
                let b1 = build_ring[c.idx];
                let b2 = build_ring[(c.idx + 1) % n_build];
                if edges_intersect_general(a1, a2, b1, b2, eps) {
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
                }
            });
            if found.is_break() {
                return true;
            }
        }
    }
    #[cfg(not(feature = "rstar"))]
    {
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
    }
    false
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

pub(crate) fn check_holes_valid(
    shell: &[Coord<f64>],
    interiors: &[LineString<f64>],
) -> Vec<GeometryValidationError> {
    let mut errors = Vec::new();

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

    for hole in interiors {
        // Check if hole edges cross the shell boundary (hole not fully inside)
        if check_rings_intersect(&hole.0[..], shell, eps) {
            errors.push(GeometryValidationError::HoleOutsideShell);
            return errors;
        }

        // A hole touching the shell at ≥ 2 distinct points may disconnect the interior.
        // Note: the same vertex can be on 2+ edges of the shell (outgoing + incoming),
        // so we must deduplicate touch points.
        let mut touch_count = 0usize;
        let mut seen_touches: Vec<Coord<f64>> = Vec::new();
        for &hp in &hole.0 {
            if point_on_ring(hp, shell, eps) && !seen_touches.contains(&hp) {
                touch_count += 1;
                seen_touches.push(hp);
            }
        }
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
        #[cfg(feature = "rstar")]
        {
            struct HoleEnv2 {
                idx: usize,
                env: rstar::AABB<[f64; 2]>,
            }
            impl rstar::RTreeObject for HoleEnv2 {
                type Envelope = rstar::AABB<[f64; 2]>;
                fn envelope(&self) -> Self::Envelope {
                    self.env
                }
            }
            let mut envs = Vec::with_capacity(holes.len());
            for (i, h) in holes.iter().enumerate() {
                let first = h.first().map(|c| (c.x, c.y)).unwrap_or((0.0, 0.0));
                let (mut min_x, mut max_x, mut min_y, mut max_y) =
                    (first.0, first.0, first.1, first.1);
                for c in *h {
                    min_x = min_x.min(c.x);
                    max_x = max_x.max(c.x);
                    min_y = min_y.min(c.y);
                    max_y = max_y.max(c.y);
                }
                envs.push(HoleEnv2 {
                    idx: i,
                    env: rstar::AABB::from_corners([min_x, min_y], [max_x, max_y]),
                });
            }
            let tree = rstar::RTree::bulk_load(envs);
            for (i, h2) in holes.iter().enumerate() {
                let Some(pt) = h2.first().copied() else {
                    continue;
                };
                let query = rstar::AABB::from_corners([pt.x, pt.y], [pt.x, pt.y]);
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
                    errors.push(GeometryValidationError::NestedHoles);
                    return errors;
                }
            }
        }
        #[cfg(not(feature = "rstar"))]
        {
            for i in 0..holes.len() {
                for j in 0..holes.len() {
                    if i == j {
                        continue;
                    }
                    if let Some(pt) = holes[j].first().copied()
                        && point_in_ring_exclusive(pt, holes[i]) {
                        errors.push(GeometryValidationError::NestedHoles);
                        return errors;
                    }
                }
            }
        }
    }
    errors
}

impl GeoValidation for Point<f64> {
    type Scalar = f64;

    fn validate(&self) -> ValidationResult {
        // Point(NaN, NaN) is the geo representation of POINT EMPTY — valid OGC
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
pub(crate) fn check_linestring_self_intersection(coords: &[Coord<f64>]) -> bool {
    let n = coords.len() - 1;
    if n < 3 {
        return false;
    }
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

    // Brute force for small inputs
    if n <= 64 {
        for i in 0..n {
            let a1 = coords[i];
            let a2 = coords[i + 1];
            for j in i + 2..n {
                let b1 = coords[j];
                let b2 = coords[j + 1];
                if edges_intersect_general(a1, a2, b1, b2, eps) {
                    return true;
                }
            }
        }
        return false;
    }

    #[cfg(feature = "rstar")]
    {
        let tree = build_ls_edge_tree(coords);
        for i in 0..n {
            let a1 = coords[i];
            let a2 = coords[i + 1];
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
                let j = c.idx;
                if j <= i + 1 {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }
                let b1 = coords[j];
                let b2 = coords[j + 1];
                if edges_intersect_general(a1, a2, b1, b2, eps) {
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
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
            let a1 = coords[i];
            let a2 = coords[i + 1];
            for j in i + 2..n {
                let b1 = coords[j];
                let b2 = coords[j + 1];
                if edges_intersect_general(a1, a2, b1, b2, eps) {
                    return true;
                }
            }
        }
        false
    }
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

    // Brute force when both components are small
    if n1.max(n2) <= 64 {
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
                if edges_intersect_general(a1, a2, b1, b2, eps) {
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
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
// Free functions — convenience wrappers around GeoValidation
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

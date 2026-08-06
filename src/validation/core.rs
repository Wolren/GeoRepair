use geo::{Coord, GeoFloat, Line, LineString, MultiLineString, MultiPoint, Point, Rect, Triangle};
use std::rc::Rc;
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

    /// Quick validity check - returns `true` if the geometry passes all OGC rules.
    fn is_valid(&self) -> bool {
        self.validate().valid
    }

    /// Full validation - returns a [`ValidationResult`] with all violations found.
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

/// Padded 2D bounding-box overlap for an edge pair (the R-tree envelope
/// filter equivalent: both boxes inflated by 1e-10 x max-dim, floored at
/// 1e-10). Conservative by construction: any pair the exact predicates
/// accept has overlapping boxes, so rejecting here never changes results.
#[inline]
fn padded_bbox_overlap(a1: Coord<f64>, a2: Coord<f64>, b1: Coord<f64>, b2: Coord<f64>) -> bool {
    let (lo_x, hi_x) = if a1.x < a2.x { (a1.x, a2.x) } else { (a2.x, a1.x) };
    let (lo_y, hi_y) = if a1.y < a2.y { (a1.y, a2.y) } else { (a2.y, a1.y) };
    let (lo_x2, hi_x2) = if b1.x < b2.x { (b1.x, b2.x) } else { (b2.x, b1.x) };
    let (lo_y2, hi_y2) = if b1.y < b2.y { (b1.y, b2.y) } else { (b2.y, b1.y) };
    let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
    let ext2 = (hi_x2 - lo_x2).abs().max((hi_y2 - lo_y2).abs()).max(1.0) * 1e-10;
    hi_x + ext >= lo_x2 - ext2
        && lo_x - ext <= hi_x2 + ext2
        && hi_y + ext >= lo_y2 - ext2
        && lo_y - ext <= hi_y2 + ext2
}

/// Order-preserving u64 encoding of an f64 (IEEE bit trick): positives map
/// above negatives, negatives reverse-magnitude. NaN handled upstream
/// (finite check in check_ring_validity).
#[inline]
fn sortable_u64(x: f64) -> u64 {
    let bits = x.to_bits();
    if bits >> 63 == 0 {
        bits | 0x8000_0000_0000_0000
    } else {
        !bits
    }
}

thread_local! {
    static SWEEP_SCRATCH: std::cell::RefCell<SweepScratch> =
        std::cell::RefCell::new(SweepScratch::default());
}

struct SweepScratch {
    keys: Vec<u64>,
    order: Vec<u32>,
    tmp_keys: Vec<u64>,
    tmp_order: Vec<u32>,
    counts: Box<[u32; 256]>,
    /// Padded bounds per ring edge: [lo_x, hi_x, lo_y, hi_y].
    spans: Vec<[f64; 4]>,
    active: Vec<u32>,
}

impl Default for SweepScratch {
    fn default() -> Self {
        SweepScratch {
            keys: Vec::new(),
            order: Vec::new(),
            tmp_keys: Vec::new(),
            tmp_order: Vec::new(),
            counts: Box::new([0u32; 256]),
            spans: Vec::new(),
            active: Vec::new(),
        }
    }
}

/// LSD radix sort of (keys, order) pairs, 8 bits x 8 passes. Stable; sorts
/// by ascending key. Buffers are passed in (the caller already holds the
/// TLS scratch borrow - re-borrowing would panic).
fn radix_sort_u64(
    keys: &mut Vec<u64>,
    order: &mut Vec<u32>,
    tmp_keys: &mut Vec<u64>,
    tmp_order: &mut Vec<u32>,
    counts: &mut [u32; 256],
) {
    let n = keys.len();
    tmp_keys.resize(n, 0);
    tmp_order.resize(n, 0);
    for shift in (0..64).step_by(8) {
        counts.fill(0);
        for &k in keys.iter() {
            counts[((k >> shift) & 0xff) as usize] += 1;
        }
        let mut acc = 0u32;
        for c in counts.iter_mut() {
            let t = *c;
            *c = acc;
            acc += t;
        }
        for i in 0..n {
            let k = keys[i];
            let b = ((k >> shift) & 0xff) as usize;
            let pos = counts[b] as usize;
            tmp_keys[pos] = k;
            tmp_order[pos] = order[i];
            counts[b] += 1;
        }
        std::mem::swap(tmp_keys, keys);
        std::mem::swap(tmp_order, order);
    }
}

/// Radix sort of (keys, order) using the TLS scratch buffers (no caller
/// borrow held). Used for the cycle-detection vertex lists.
fn radix_sort_keys_tls(keys: &mut Vec<u64>, order: &mut Vec<u32>) {
    SWEEP_SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        let SweepScratch {
            tmp_keys,
            tmp_order,
            counts,
            ..
        } = &mut *s;
        radix_sort_u64(keys, order, tmp_keys, tmp_order, counts);
    });
}

/// Rings whose x-overlap active set exceeds this route to the spatial
/// tree / brute force instead of the linear-active sweep (which would be
/// O(n^2) on them). Measured worst real-world giant: 63.
const SWEEP_DENSE_ACTIVE_LIMIT: usize = 256;

/// Self-intersection sweep. Returns Some(true) on intersection, Some(false)
/// when clean, None when the ring's x-overlap density exceeds the routing
/// limit (caller falls back to the indexed / brute path).
fn sweep_ring_self_intersects(ring: &[Coord<f64>], eps: f64) -> Option<bool> {
    let n = ring.len() - 1;
    SWEEP_SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        let SweepScratch {
            keys,
            order,
            spans,
            active,
            tmp_keys,
            tmp_order,
            counts,
        } = &mut *s;
        keys.clear();
        order.clear();
        spans.clear();
        keys.reserve(n);
        order.reserve(n);
        spans.reserve(n);
        for i in 0..n {
            let a = ring[i];
            let b = ring[(i + 1) % n];
            let (lo_x, hi_x) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
            let (lo_y, hi_y) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
            let ext = (hi_x - lo_x).abs().max((hi_y - lo_y).abs()).max(1.0) * 1e-10;
            keys.push(sortable_u64(lo_x - ext));
            order.push(i as u32);
            spans.push([lo_x - ext, hi_x + ext, lo_y - ext, hi_y + ext]);
        }
        radix_sort_u64(keys, order, tmp_keys, tmp_order, counts);
        active.clear();
        for pos in 0..n {
            let r_i = order[pos] as usize;
            let cur = spans[r_i];
            // Limit check BEFORE the retain: a pathological ring with a
            // growing active set would otherwise pay O(n x active) in
            // retains before ever reaching the check.
            if active.len() > SWEEP_DENSE_ACTIVE_LIMIT {
                return None;
            }
            active.retain(|&p| spans[p as usize][1] >= cur[0]);
            for &p in &*active {
                let t = spans[p as usize];
                // x-overlap is guaranteed by the retain condition; only the
                // y gate remains (matches the tree's 2D envelope filter).
                if t[3] < cur[2] || t[2] > cur[3] {
                    continue;
                }
                let r_j = p as usize;
                if r_i.abs_diff(r_j) <= 1 {
                    continue;
                }
                if check_edge_pair_intersection(ring, r_i, r_j, eps) {
                    return Some(true);
                }
            }
            active.push(r_i as u32);
        }
        Some(false)
    })
}

/// R-tree self-intersection check (the dense-ring fallback; kept exact -
/// same predicate and pair rules as the sweep).
#[cfg(feature = "rstar")]
fn ring_tree_self_intersects(ring: &[Coord<f64>], n: usize, eps: f64) -> bool {
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
            env: rstar::AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext]),
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
        let env = rstar::AABB::from_corners([lo_x - ext, lo_y - ext], [hi_x + ext, hi_y + ext]);
        let found = tree.locate_in_envelope_intersecting_int(env, |c| {
            let j = c.idx as usize;
            if j <= i {
                return std::ops::ControlFlow::Continue(());
            }
            if i.abs_diff(j) <= 1 {
                return std::ops::ControlFlow::Continue(());
            }
            if check_edge_pair_intersection(ring, i, j, eps) {
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

pub(crate) fn edges_intersect_general(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
) -> bool {
    // Robust (Shewchuk adaptive) orient2d. The fast f64 predicate flips
    // signs on mixed-magnitude inputs (e.g. 1e-10-scale edges against an
    // 8.4e7-scale ring: fast orient2d gave -6.25e-2 / +6.25e-2 for two
    // genuinely non-crossing segments, producing a false SelfIntersection
    // that GEOS does not report - measured: mixed4 fuzz seed). Shewchuk's
    // adaptive version returns the exact sign for the same cost when the
    // f64 computation is exact (the common case).
    let o1 = crate::orient::orient2d(a1, a2, b1);
    let o2 = crate::orient::orient2d(a1, a2, b2);
    let o3 = crate::orient::orient2d(b1, b2, a1);
    let o4 = crate::orient::orient2d(b1, b2, a2);

    // Proper crossing. Zero-safe strict opposite sign: the product form
    // (o1 * o2 < 0.0) treats a -0.0 orient as negative, flagging a
    // collinear touch as a crossing (false SelfIntersection on valid
    // geometry with -0.0 coordinates; measured: this inflated the
    // real-world "2,298 invalid" class: with the zero-safe form the
    // winding-agnostic count is 1). Matches the sweep predicates
    // (segments_properly_cross / segments_properly_cross_seg).
    if (o1 > 0.0 && o2 < 0.0 || o1 < 0.0 && o2 > 0.0)
        && (o3 > 0.0 && o4 < 0.0 || o3 < 0.0 && o4 > 0.0)
    {
        return true;
    }

    // Collinear overlap (excluding endpoint-only touching). The collinearity
    // tolerance must be RELATIVE to the pair's own edge lengths: orient2d
    // magnitudes are O(L²) (twice the triangle area). The constant sits at
    // the f64 noise floor — `32 * EPSILON * L²` covers ~32 ulps of
    // coordinate rounding — NOT the historical `1e-12 * L²`, which is a
    // perpendicular-distance tolerance of `1e-12 * L` and swallows genuinely
    // separated near-parallel edges (measured: invariant_sliver_hole seed
    // cc 9b38e427, scale=51.29, sliver_width=1e-12 — parallel edges 1e-12
    // apart gave exact orients 2.05e-11 < 4.2e-10 and were flagged as
    // overlapping, a false SelfIntersection on input GEOS validates). The
    // caller's eps (1e-12 * bbox scale, floored at 1.0) is an ABSOLUTE
    // length that exceeds the exact orient of genuinely non-collinear
    // near-parallel sliver edges at large coordinate magnitude (measured:
    // coord_wrap_around seed base=-9607183.16, step=5.47e-4, n=7 -> exact
    // orients 8.2e-13/3.1e-13 vs caller eps 1e-12 flagged a false
    // SelfIntersection on a MultiPolygon GEOS validates bit-for-bit).
    let la2 = (a2.x - a1.x).powi(2) + (a2.y - a1.y).powi(2);
    let lb2 = (b2.x - b1.x).powi(2) + (b2.y - b1.y).powi(2);
    let collinear_eps = 32.0 * f64::EPSILON * la2.max(lb2);
    let collinear = o1.abs() <= collinear_eps && o2.abs() <= collinear_eps;
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
        } else if len2 > 0.0 && o1 == 0.0 && o2 == 0.0 {
            // EXACT collinearity below the length gate. o1/o2 exactly zero
            // means the endpoints lie bit-exactly on the other edge's line —
            // real shared topology (e.g. two MultiPolygon components sharing
            // a sub-grid edge after snap rounding), not near-collinear
            // rounding noise. The length gate exists for slivers whose
            // orient is within ulps of zero; exact-zero orientation is a
            // deliberate touch and must be flagged regardless of scale.
            // Measured: mixed-magnitude polygon (1e-9..5e6) whose repaired
            // components shared a 1e-8 edge; the global eps (1e-12 * 5.2e6
            // ≈ 5.2e-6) swallowed it and GEOS flagged the result as
            // Self-intersection. Differential fuzz found it.
            let t1 = ((b1.x - a1.x) * dx + (b1.y - a1.y) * dy) / len2;
            let t2 = ((b2.x - a1.x) * dx + (b2.y - a1.y) * dy) / len2;
            let lo = 0.0f64.max(t1.min(t2));
            let hi = 1.0f64.min(t1.max(t2));
            if hi - lo > 0.0 {
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
    if edges_intersect_general(a1, a2, b1, b2, eps) {
        return true;
    }
    // Ring self-touch at a vertex lying on a non-adjacent edge (T-junction):
    // proper-crossing and collinear-overlap miss it, GEOS IsValidOp rejects
    // it (Test 22: closing vertex (110 140) on edge (60 90)-(160 190)).
    // Same-ring pairs only - this function is only called from
    // check_ring_validity. Cross-ring T-junctions (hole vertex on shell
    // edge) are VALID OGC touches and must never be flagged.
    edges_vertex_on_edge(a1, a2, b1, b2)
}

/// Strict-interior vertex-on-edge touch between two segments: an endpoint
/// of one segment lying strictly on the interior of the other. Endpoint
/// equality is excluded (shared vertices are handled by the pinch/adjacency
/// logic). Bbox-gated before the robust orient tests so clean data pays
/// only 4 comparisons per pair. #[inline] is REQUIRED: this runs inside the
/// per-pair small-ring sweep on the 1.58M-poly hot path (measured: a
/// non-inlined call cost +25% on the full dataset).
///
/// Tolerance is segment-local (`1e-12 * len²` of the tested segment, see
/// [`point_strictly_on_segment`]): orient2d magnitudes are O(L²) of that
/// segment, and the strict-interior margin must stay tiny relative to it.
/// A pair-max tolerance inflates past micro segments in mixed-magnitude
/// rings (measured: differential fuzz 2026-08-03; small_ring_equiv seed 85
/// documents the same class at the small end).
#[inline]
pub(crate) fn edges_vertex_on_edge(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
) -> bool {
    let (lo_x, hi_x) = if a1.x < a2.x { (a1.x, a2.x) } else { (a2.x, a1.x) };
    let (lo_y, hi_y) = if a1.y < a2.y { (a1.y, a2.y) } else { (a2.y, a1.y) };
    let (lo_x2, hi_x2) = if b1.x < b2.x { (b1.x, b2.x) } else { (b2.x, b1.x) };
    let (lo_y2, hi_y2) = if b1.y < b2.y { (b1.y, b2.y) } else { (b2.y, b1.y) };
    if hi_x < lo_x2 || lo_x > hi_x2 || hi_y < lo_y2 || lo_y > hi_y2 {
        return false;
    }
    point_strictly_on_segment(a1, b1, b2)
        || point_strictly_on_segment(a2, b1, b2)
        || point_strictly_on_segment(b1, a1, a2)
        || point_strictly_on_segment(b2, a1, a2)
}

/// True if `p` lies strictly on the interior of segment (a, b): on the
/// segment's line (robust orient within eps) and strictly between the
/// endpoints. Endpoint equality returns false.
///
/// The tolerance is computed from the SEGMENT ITSELF (`1e-12 * len²`), not
/// the pair: orient2d magnitudes are O(L²) of the tested segment, and the
/// strict-interior bbox margin must stay tiny relative to that segment.
/// Using the pair's larger edge inflates the margin past micro segments —
/// measured: a mixed-magnitude repaired ring whose 3e-8 closing edge was
/// crossed by a vertex of a 2.3e6-scale edge; the pair-max eps (1e-12 *
/// 1.8e13 ≈ 18) made the strict-interior test vacuous and GEOS flagged
/// Ring Self-intersection[1e-08 -1e-08] that we accepted (differential
/// fuzz 2026-08-03).
fn point_strictly_on_segment(p: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> bool {
    if p == a || p == b {
        return false;
    }
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let eps = 1e-12 * (dx * dx + dy * dy);
    let o = crate::orient::orient2d(a, b, p);
    if o.abs() > eps {
        return false;
    }
    let (lo_x, hi_x) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
    let (lo_y, hi_y) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
    // Strict interior on at least one axis (axis-aligned segments have a
    // constant axis; diagonal segments satisfy both).
    (p.x > lo_x + eps && p.x < hi_x - eps) || (p.y > lo_y + eps && p.y < hi_y - eps)
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

    // Brute-force when both rings are small - faster than building a tree.
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
fn point_on_segment_exact(pt: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> bool {
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
    let shell_tree: Option<Rc<rstar::RTree<EdgeIdx>>> = (shell.len() - 1 > 64)
        .then(|| Rc::new(build_ring_edge_tree(shell)));

    for hole in interiors {
        // Check if hole edges cross the shell boundary (hole not fully inside)
        #[cfg(feature = "rstar")]
        let crossing = match &shell_tree {
            Some(tree) => ring_edges_intersect_tree(&hole.0[..], shell, tree, eps),
            None => check_rings_intersect(&hole.0[..], shell, eps),
        };
        #[cfg(not(feature = "rstar"))]
        let crossing = check_rings_intersect(&hole.0[..], shell, eps);
        if crossing {
            errors.push(GeometryValidationError::HoleOutsideShell);
            return errors;
        }

        // A hole touching the shell at >= 2 distinct points may disconnect
        // the interior. Note: the same vertex can be on 2+ edges of the
        // shell (outgoing + incoming), so we must deduplicate touch points.
        // Touch test is EXACT (point_on_segment_exact): tolerance-based
        // touches fabricated near-miss contacts on real cadastral giants
        // (GEOS isValid=true; measured 2026-08-06).
        let mut touch_count = 0usize;
        let mut seen_touches: Vec<Coord<f64>> = Vec::new();
        #[cfg(feature = "rstar")]
        let hole_touches: Vec<Coord<f64>> = match &shell_tree {
            Some(tree) => ring_touch_points(&hole.0[..], shell, tree),
            None => hole
                .0
                .iter()
                .copied()
                .filter(|&hp| point_on_ring(hp, shell, 0.0))
                .collect(),
        };
        #[cfg(not(feature = "rstar"))]
        let hole_touches: Vec<Coord<f64>> = hole
            .0
            .iter()
            .copied()
            .filter(|&hp| point_on_ring(hp, shell, 0.0))
            .collect();
        for hp in hole_touches {
            if !seen_touches.contains(&hp) {
                touch_count += 1;
                seen_touches.push(hp);
            }
        }
        if touch_count >= 2 {
            errors.push(GeometryValidationError::DisconnectedInteriorRing);
            return errors;
        }

        // If no hole vertex is strictly inside the shell, the hole is
        // entirely outside. Single-point tangent touches (touch_count == 1)
        // are valid per OGC. Tree-accelerated: the naive per-vertex
        // point_in_ring_exclusive paid O(|hole| x |shell|) on giant shells.
        #[cfg(feature = "rstar")]
        let any_inside = match &shell_tree {
            Some(tree) => hole
                .0
                .iter()
                .any(|&hp| point_in_ring_exclusive_tree(hp, shell, tree, max_x)),
            None => hole.0.iter().any(|&hp| point_in_ring_exclusive(hp, shell)),
        };
        #[cfg(not(feature = "rstar"))]
        let any_inside = hole.0.iter().any(|&hp| point_in_ring_exclusive(hp, shell));
        if !any_inside {
            errors.push(GeometryValidationError::HoleOutsideShell);
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

fn bbox_overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Does box `a` fully cover box `b`? (GEOS envelope-covers gate.)
fn bbox_covers(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[0] && a[2] >= b[2] && a[1] <= b[1] && a[3] >= b[3]
}

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
        for i in min_ring..n {
            let q = rstar::AABB::from_corners([bboxes[i][0], bboxes[i][1]], [bboxes[i][2], bboxes[i][3]]);
            let _ = tree.locate_in_envelope_intersecting_int(q, |c| {
                if c.idx > i {
                    out.push((i, c.idx));
                }
                std::ops::ControlFlow::<(), ()>::Continue(())
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
        if p1.y <= pt.y {
            if p2.y > pt.y {
                if o > 0.0 {
                    wn += 1;
                }
            }
        } else if p2.y <= pt.y {
            if o < 0.0 {
                wn -= 1;
            }
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
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
                }
            })
            .is_break();
        if hit {
            out.push(v);
        }
    }
    out
}

/// Ring-touch graph cycle detection (GEOS PolygonRing::findHoleCycleLocation
/// + scanForHoleCycle port). Touches = exact shared vertices + vertex-on-
/// edge contacts within eps. Returns true when the touch graph contains a
/// cycle through pairwise-DISTINCT coordinates (disconnected interior);
/// touches at a single coordinate never close a cycle (GEOS isValid=true
/// for multiple holes meeting at one point).
///
/// Per-ring structures (sorted unique vertex list, edge tree for large
/// rings) are built ONCE outside the pair loop - never re-sorted or
/// re-indexed per pair (the naive version cost 170x on the real-world
/// giants, measured 2026-08-06).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn detect_hole_cycle(
    rings: &[&[Coord<f64>]],
    bboxes: &[[f64; 4]],
    eps: f64,
    #[cfg(feature = "rstar")] shell_tree: Option<&Rc<rstar::RTree<EdgeIdx>>>,
) -> bool {
    let n = rings.len();
    // Per-ring structures built ONCE (never per pair): a sorted unique
    // vertex list (x-range probing only - shared vertices come from the
    // by_coord pass below; the shell's shared vertices are additionally
    // covered by ring_touch_points in check_holes_valid) and an edge tree
    // for large rings. The shell's tree is REUSED from check_holes_valid
    // (rings[0] is the shell) - building a second rstar bulk_load of a
    // 600k-edge shell cost ~100ms per giant (measured 2026-08-06). The
    // vertex lists are radix-sorted by x (std sort cost ~50ms on the
    // giant shells; measured 2026-08-06).
    let mut sorted: Vec<Vec<Coord<f64>>> = Vec::with_capacity(n);
    for r in rings {
        let mut v = r.to_vec();
        let mut keys: Vec<u64> = v.iter().map(|c| sortable_u64(c.x)).collect();
        let mut order: Vec<u32> = (0..v.len() as u32).collect();
        radix_sort_keys_tls(&mut keys, &mut order);
        let mut perm: Vec<Coord<f64>> = Vec::with_capacity(v.len());
        for &i in &order {
            perm.push(v[i as usize]);
        }
        v = perm;
        sorted.push(v);
    }
    #[cfg(feature = "rstar")]
    let edge_trees: Vec<Option<Rc<rstar::RTree<EdgeIdx>>>> = rings
        .iter()
        .enumerate()
        .map(|(i, r)| {
            if i == 0 {
                shell_tree.cloned()
            } else if r.len() - 1 > 64 {
                Some(Rc::new(build_ring_edge_tree(r)))
            } else {
                None
            }
        })
        .collect();
    let mut touches: Vec<Vec<(usize, Coord<f64>)>> = vec![Vec::new(); n];
    // Exact shared vertices among the HOLES: one global pass (vertex ->
    // containing rings), O(total vertices); -0.0 normalizes to 0.0 so both
    // spellings match. The shell (ring 0) is skipped: its shared vertices
    // are hole vertices lying on shell edges, already collected exactly by
    // ring_touch_points in check_holes_valid (the shell's vertex-on-edge
    // probes below are kept for shell vertices on hole edges). Skipping the
    // shell removes ~600k hash inserts per giant (measured 2026-08-06).
    let mut by_coord: rustc_hash::FxHashMap<(u64, u64), Vec<usize>> =
        rustc_hash::FxHashMap::with_capacity_and_hasher(64, Default::default());
    for (ri, ring) in rings.iter().enumerate().skip(1) {
        for &v in *ring {
            let k = (
                if v.x == 0.0 { 0u64 } else { v.x.to_bits() },
                if v.y == 0.0 { 0u64 } else { v.y.to_bits() },
            );
            by_coord.entry(k).or_default().push(ri);
        }
    }
    for (&k, rs) in by_coord.iter() {
        if rs.len() < 2 {
            continue;
        }
        let c = Coord {
            x: f64::from_bits(k.0),
            y: f64::from_bits(k.1),
        };
        for (i, &a) in rs.iter().enumerate() {
            for &b in rs.iter().skip(i + 1) {
                if a != b {
                    touches[a].push((b, c));
                    touches[b].push((a, c));
                }
            }
        }
    }
    // Vertex-on-edge contacts, both directions (duplicates deduped below).
    // Each probe iterates only the source vertices inside the target's x
    // range (binary search over the precomputed sorted list), so giant
    // shells probe each hole in O(log V) + near-hole vertices.
    for (a, b) in overlap_pairs(bboxes, 0) {
        let ra = rings[a];
        let rb = rings[b];
        let n_a = ra.len() - 1;
        let n_b = rb.len() - 1;
        if n_a > 64 || n_b > 64 {
            #[cfg(feature = "rstar")]
            {
                collect_on_edge_tree(a, b, &sorted[a], rb, bboxes, eps, &edge_trees, &mut touches);
                collect_on_edge_tree(b, a, &sorted[b], ra, bboxes, eps, &edge_trees, &mut touches);
            }
            #[cfg(not(feature = "rstar"))]
            {
                collect_on_edge_brute(a, b, &sorted[a], rb, bboxes, eps, &mut touches);
                collect_on_edge_brute(b, a, &sorted[b], ra, bboxes, eps, &mut touches);
            }
        } else {
            collect_on_edge_brute(a, b, &sorted[a], rb, bboxes, eps, &mut touches);
            collect_on_edge_brute(b, a, &sorted[b], ra, bboxes, eps, &mut touches);
        }
    }
    for t in touches.iter_mut() {
        t.sort_by(|x, y| {
            x.1.x
                .total_cmp(&y.1.x)
                .then(x.1.y.total_cmp(&y.1.y))
                .then(x.0.cmp(&y.0))
        });
        t.dedup_by(|x, y| x.0 == y.0 && x.1.x == y.1.x && x.1.y == y.1.y);
    }
    // GEOS findHoleCycleLocation: per-root DFS over the touch graph; a ring
    // reached through two different touch paths closes a cycle.
    let mut touch_set_root: Vec<Option<usize>> = vec![None; n];
    let mut stack: Vec<(usize, Coord<f64>)> = Vec::new();
    for root in 0..n {
        if touch_set_root[root].is_some() {
            continue;
        }
        touch_set_root[root] = Some(root);
        if touches[root].is_empty() {
            continue;
        }
        // Init: push ALL of root's touches (GEOS init; nothing is marked
        // yet, so double-touches of the same ring at different coordinates
        // both enter the stack and close a cycle on the second scan).
        for &(other, coord) in &touches[root] {
            stack.push((other, coord));
        }
        while let Some((ring, entry)) = stack.pop() {
            for &(other, coord) in &touches[ring] {
                if coord.x == entry.x && coord.y == entry.y {
                    continue;
                }
                if touch_set_root[other] == Some(root) {
                    return true;
                }
                if touch_set_root[other].is_none() {
                    touch_set_root[other] = Some(root);
                    stack.push((other, coord));
                }
            }
        }
    }
    false
}

/// Brute-force vertex-on-edge touch collection (small rings). Iterates only
/// the source vertices inside the target's x range (binary search over the
/// precomputed sorted list) and the target's bbox - giant shells probe each
/// hole in O(log V) + near-hole vertices.
fn collect_on_edge_brute(
    src: usize,
    tgt: usize,
    src_sorted: &[Coord<f64>],
    tgt_ring: &[Coord<f64>],
    bboxes: &[[f64; 4]],
    eps: f64,
    touches: &mut [Vec<(usize, Coord<f64>)>],
) {
    let tb = bboxes[tgt];
    let n_t = tgt_ring.len().saturating_sub(1);
    if n_t < 1 {
        return;
    }
    let lo = src_sorted.partition_point(|v| v.x < tb[0] - eps);
    let hi = src_sorted.partition_point(|v| v.x <= tb[2] + eps);
    for &v in &src_sorted[lo..hi] {
        if v.y < tb[1] - eps || v.y > tb[3] + eps {
            continue;
        }
        let mut hit = false;
        for i in 0..n_t {
            // EXACT on-edge test: a touch is a zero-distance contact (GEOS
            // parity, see point_on_segment_exact). The tolerance version
            // fabricated touches from near-miss vertices in real cadastral
            // giants (GEOS isValid=true, measured 2026-08-06).
            if point_on_segment_exact(v, tgt_ring[i], tgt_ring[(i + 1) % n_t]) {
                hit = true;
                break;
            }
        }
        if hit {
            touches[src].push((tgt, v));
            touches[tgt].push((src, v));
        }
    }
}

/// Edge-tree vertex-on-edge touch collection (large target ring). The
/// source side iterates only x-range-gated vertices (binary search over the
/// precomputed sorted list); small target rings fall back to brute force
/// over their edges for the few in-range vertices.
#[cfg(feature = "rstar")]
fn collect_on_edge_tree(
    src: usize,
    tgt: usize,
    src_sorted: &[Coord<f64>],
    tgt_ring: &[Coord<f64>],
    bboxes: &[[f64; 4]],
    eps: f64,
    edge_trees: &[Option<Rc<rstar::RTree<EdgeIdx>>>],
    touches: &mut [Vec<(usize, Coord<f64>)>],
) {
    let tb = bboxes[tgt];
    let n_t = tgt_ring.len().saturating_sub(1);
    if n_t < 1 {
        return;
    }
    let lo = src_sorted.partition_point(|v| v.x < tb[0] - eps);
    let hi = src_sorted.partition_point(|v| v.x <= tb[2] + eps);
    let tree = match &edge_trees[tgt] {
        Some(t) => t,
        None => {
            // Small target ring: brute-force its edges for the few source
            // vertices in its x range and bbox. EXACT on-edge test - see
            // collect_on_edge_brute for the parity note (eps fabricated
            // touches on real cadastral giants, GEOS isValid=true).
            for &v in &src_sorted[lo..hi] {
                if v.y < tb[1] - eps || v.y > tb[3] + eps {
                    continue;
                }
                for i in 0..n_t {
                    if point_on_segment_exact(v, tgt_ring[i], tgt_ring[(i + 1) % n_t]) {
                        touches[src].push((tgt, v));
                        touches[tgt].push((src, v));
                        break;
                    }
                }
            }
            return;
        }
    };
    for &v in &src_sorted[lo..hi] {
        if v.y < tb[1] - eps || v.y > tb[3] + eps {
            continue;
        }
        let q = rstar::AABB::from_corners([v.x - eps, v.y - eps], [v.x + eps, v.y + eps]);
        let hit = tree
            .locate_in_envelope_intersecting_int(q, |c| {
                if point_on_segment_exact(v, tgt_ring[c.idx], tgt_ring[(c.idx + 1) % n_t]) {
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::<(), ()>::Continue(())
                }
            })
            .is_break();
        if hit {
            touches[src].push((tgt, v));
            touches[tgt].push((src, v));
        }
    }
}

/// First ring vertex after `p0` that is not exactly equal to it.
fn first_non_equal(ring: &[Coord<f64>], p0: Coord<f64>) -> Option<Coord<f64>> {
    ring.iter()
        .skip(1)
        .find(|&&c| c.x != p0.x || c.y != p0.y)
        .copied()
}

/// GEOS PolygonTopologyAnalyzer::isRingNested port: is `test` nested inside
/// `target`? A start point strictly inside => yes; outside => no; on the
/// boundary => the incident-segment topology decides.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn is_ring_nested(test: &[Coord<f64>], target: &[Coord<f64>], eps: f64) -> bool {
    let Some(&p0) = test.first() else {
        return false;
    };
    if target.len() < 4 {
        return false;
    }
    if point_on_ring(p0, target, eps) {
        let Some(p1) = first_non_equal(test, p0) else {
            return false;
        };
        return is_incident_segment_in_ring(p0, p1, target, eps);
    }
    point_in_ring_exclusive(p0, target)
}

/// GEOS PolygonTopologyAnalyzer::isIncidentSegmentInRing port: is the
/// segment p0->p1 (p0 on the ring boundary) inside the ring at the node?
fn is_incident_segment_in_ring(
    p0: Coord<f64>,
    p1: Coord<f64>,
    target: &[Coord<f64>],
    eps: f64,
) -> bool {
    let n = target.len() - 1;
    if n < 2 {
        return false;
    }
    // Segment containing p0 (GEOS intersectingSegIndex: p0 == segment end
    // selects the NEXT segment start).
    let mut idx = 0usize;
    for i in 0..n {
        if point_on_segment(p0, target[i], target[(i + 1) % n], eps) {
            idx = if p0.x == target[i + 1].x && p0.y == target[i + 1].y {
                i + 1
            } else {
                i
            };
            break;
        }
    }
    if idx >= n {
        idx = 0;
    }
    // Prev/next ring vertices, walking away from coordinates equal to p0.
    let mut i_prev = idx;
    for _ in 0..n {
        if target[i_prev].x != p0.x || target[i_prev].y != p0.y {
            break;
        }
        i_prev = (i_prev + n - 1) % n;
    }
    let mut i_next = idx;
    for _ in 0..n {
        if target[i_next].x != p0.x || target[i_next].y != p0.y {
            break;
        }
        i_next = (i_next + 1) % n;
    }
    let (mut a0, mut a1) = (target[i_prev], target[i_next]);
    // GEOS: interior on the right for CW rings; CCW rings swap prev/next so
    // the corner is traversed with the interior wedge on the left.
    if crate::util::robust_is_ccw(target) {
        std::mem::swap(&mut a0, &mut a1);
    }
    is_interior_segment(p0, a0, a1, p1)
}

/// GEOS PolygonNodeTopology::isInteriorSegment port.
fn is_interior_segment(node: Coord<f64>, a0: Coord<f64>, a1: Coord<f64>, b: Coord<f64>) -> bool {
    let (mut a_lo, mut a_hi) = (a0, a1);
    let mut is_interior_between = true;
    if is_angle_greater(node, a_lo, a_hi) {
        std::mem::swap(&mut a_lo, &mut a_hi);
        is_interior_between = false;
    }
    let b_between = is_between(node, b, a_lo, a_hi);
    (b_between && is_interior_between) || (!b_between && !is_interior_between)
}

/// GEOS Quadrant + isAngleGreater port: p > q when p is CCW of q as seen
/// from the origin.
fn is_angle_greater(origin: Coord<f64>, p: Coord<f64>, q: Coord<f64>) -> bool {
    let qp = quadrant(origin, p);
    let qq = quadrant(origin, q);
    if qp > qq {
        return true;
    }
    if qp < qq {
        return false;
    }
    // Same quadrant: p > q iff p is CCW of q (robust orient2d, > 0 = CCW).
    crate::orient::orient2d(origin, q, p) > 0.0
}

/// GEOS isBetween port: p lies in the CCW angle wedge e0 -> e1 from origin.
fn is_between(origin: Coord<f64>, p: Coord<f64>, e0: Coord<f64>, e1: Coord<f64>) -> bool {
    if !is_angle_greater(origin, p, e0) {
        return false;
    }
    !is_angle_greater(origin, p, e1)
}

/// GEOS Quadrant numbering: NE=0, NW=1, SW=2, SE=3.
fn quadrant(origin: Coord<f64>, p: Coord<f64>) -> u8 {
    let dx = p.x - origin.x;
    let dy = p.y - origin.y;
    debug_assert!(dx != 0.0 || dy != 0.0, "quadrant of zero vector");
    if dx >= 0.0 {
        if dy >= 0.0 {
            0
        } else {
            3
        }
    } else if dy >= 0.0 {
        1
    } else {
        2
    }
}

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
    if n <= 128 {
        for i in 0..n {
            for j in i + 2..n {
                if pair_intersects(i, j) {
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
                if j <= i + 1 || (closed && i == 0 && j == n - 1) {
                    return std::ops::ControlFlow::<(), ()>::Continue(());
                }
                // Vertex revisit (see the small-path pair check).
                if a1 == coords[j]
                    || a1 == coords[j + 1]
                    || a2 == coords[j]
                    || a2 == coords[j + 1]
                {
                    return std::ops::ControlFlow::Break(());
                }
                if edges_intersect_general(a1, a2, coords[j], coords[j + 1], eps)
                    || edges_vertex_on_edge(a1, a2, coords[j], coords[j + 1])
                {
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
            for j in i + 2..n {
                if pair_intersects(i, j) {
                    return true;
                }
            }
        }
        false
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
fn segments_collinear_overlap(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
) -> bool {
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

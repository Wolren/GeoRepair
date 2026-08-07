//! edge-pair intersection predicates and ring edge trees
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance:
//! core.rs was 2540 lines; the cap is 800). Content is verbatim - no
//! behavior changes; sibling modules resolve shared items through the
//! `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.



#[cfg(feature = "rstar")]
use alloc::vec::Vec;
use geo::{Coord};

pub(crate) fn edges_intersect_general(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
) -> bool {
    // Cheap bbox prefilter before the robust predicates: the vast majority
    // of pairs are envelope-disjoint, and 4 Shewchuk orient2d calls are
    // ~100 ns. Measured (2026-08-07): without this gate, the line-simplicity
    // naive loop ran the full predicate chain on every pair (2.7 ms for a
    // 500-vertex line); with it, disjoint pairs cost 4 comparisons.
    // Padded by eps: the collinear-overlap branch flags pairs separated by
    // up to eps (e.g. a 5e-14-tall sliver over a length-10 base, eps
    // 1e-11 - raw-disjoint y-ranges, genuinely within the gate; measured:
    // geo_bridge stricter_than_geo_collinear_sliver).
    {
        let (lo_x, hi_x) = if a1.x < a2.x { (a1.x, a2.x) } else { (a2.x, a1.x) };
        let (lo_y, hi_y) = if a1.y < a2.y { (a1.y, a2.y) } else { (a2.y, a1.y) };
        let (lo_x2, hi_x2) = if b1.x < b2.x { (b1.x, b2.x) } else { (b2.x, b1.x) };
        let (lo_y2, hi_y2) = if b1.y < b2.y { (b1.y, b2.y) } else { (b2.y, b1.y) };
        if hi_x < lo_x2 - eps
            || lo_x > hi_x2 + eps
            || hi_y < lo_y2 - eps
            || lo_y > hi_y2 + eps
        {
            return false;
        }
    }
    // Robust (Shewchuk adaptive) orient2d. The fast f64 predicate flips
    // signs on mixed-magnitude inputs (e.g. 1e-10-scale edges against an
    // 8.4e7-scale ring: fast orient2d gave -6.25e-2 / +6.25e-2 for two
    // genuinely non-crossing segments, producing a false SelfIntersection
    // that GEOS does not report - measured: mixed4 fuzz seed). Shewchuk's
    // adaptive version returns the exact sign for the same cost when the
    // f64 computation is exact (the common case).
    //
    // Fast-FP first (GEOS IsValidOp design): when every plain cross-product
    // orientation lies far above the relative collinear gate, its sign is
    // exact (the error is ~4 ulps of L2; the gate is 32 ulps), so the
    // crossing decision is identical to the robust predicates. Escalate to
    // Shewchuk only when some orientation is near the gate. Measured
    // (2026-08-07): the exact chain cost ~120 ns/pair; the fast path is
    // ~5 ns, and clean synthetic data pays it for ~99% of pairs (lines:
    // 269 us -> 10-15 us at 500 vertices).
    let dx_a = a2.x - a1.x;
    let dy_a = a2.y - a1.y;
    let dx_b = b2.x - b1.x;
    let dy_b = b2.y - b1.y;
    let f1 = dx_a * (b1.y - a1.y) - dy_a * (b1.x - a1.x);
    let f2 = dx_a * (b2.y - a1.y) - dy_a * (b2.x - a1.x);
    let f3 = dx_b * (a1.y - b1.y) - dy_b * (a1.x - b1.x);
    let f4 = dx_b * (a2.y - b1.y) - dy_b * (a2.x - b1.x);
    // Adaptive per-orient error bound (Shewchuk's): the fast orient's
    // rounding error is bounded by a small constant times EPS times the
    // SUM of the absolute products that make up the orient. The old
    // `32 * EPSILON * max(la2, lb2)` margin used the EDGE LENGTHS, which a
    // long edge inflates far past the orient of nearly-parallel pairs:
    // measured 2026-08-07, a 1.6e7-edge vs 219-edge pair separated by
    // 1.8e-8 gave orient 3.9e-6 against margin ~1.8 -> false collinear
    // overlap (fuzz_inprocess_loop divergence, Fast path shipped a
    // polygon the validator rejected). The product-sum bound for the same
    // pair is ~1e-20 - the orient is definitively not collinear.
    #[inline(always)]
    fn orient_err(t1: f64, t2: f64) -> f64 {
        32.0 * f64::EPSILON * (t1.abs() + t2.abs())
    }
    let e1 = orient_err(dx_a * (b1.y - a1.y), dy_a * (b1.x - a1.x));
    let e2 = orient_err(dx_a * (b2.y - a1.y), dy_a * (b2.x - a1.x));
    let e3 = orient_err(dx_b * (a1.y - b1.y), dy_b * (a1.x - b1.x));
    let e4 = orient_err(dx_b * (a2.y - b1.y), dy_b * (a2.x - b1.x));
    if f1.abs() > 2.0 * e1
        && f2.abs() > 2.0 * e2
        && f3.abs() > 2.0 * e3
        && f4.abs() > 2.0 * e4
    {
        // Zero-safe strict opposite sign (matches the exact path below).
        if (f1 > 0.0 && f2 < 0.0 || f1 < 0.0 && f2 > 0.0)
            && (f3 > 0.0 && f4 < 0.0 || f3 < 0.0 && f4 > 0.0)
        {
            return true;
        }
        return false;
    }
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
    // Exact-path collinearity gate: adaptive per-orient error bound
    // (same rationale as the fast-first gate above - the L2-based margin
    // inflates past nearly-parallel pairs when one edge is long).
    let collinear = o1.abs() <= e1 && o2.abs() <= e2;
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

/// Lean pair intersection, decision-equivalent to
/// `edges_intersect_general || edges_vertex_on_edge`, skipping the
/// vertex-on-edge chain for fast-FP-strong pairs.
///
/// The fast-FP head decides properly when every fast orient sits beyond the
/// larger of its adaptive margin and the vertex-on-edge gate (`1e-12` x the
/// TESTED segment's len2), plus the margin again for the error bound. The
/// VOE gate is ~140x the margin at equal edge lengths, so an endpoint
/// within VOE range of another segment's line can sit far outside the
/// fast-FP ambiguity zone (measured 2026-08-07: VOE flagged pairs whose
/// fast orients were ~70x beyond the margin) - the strong test MUST cover
/// both bounds or the lean path misses vertex-on-edge.
///
/// Pairs outside the strong zone set `ambiguous = true` and fall through
/// to the exact predicates (same cost as the old chain). Callers that need
/// the line-path vertex-revisit class run their equality checks only when
/// `ambiguous` is set - every revisit pair has an orient exactly zero, so
/// it always escalates (the star-comb's ~120k dense bbox pairs are all
/// strong and never pay the compares; measured 2026-08-07).
#[inline]
pub(crate) fn lean_pair_intersects(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
    ambiguous: &mut bool,
) -> bool {
    *ambiguous = false;
    // Bbox prefilter (identical to edges_intersect_general's - padded by
    // eps for the same sliver class).
    {
        let (lo_x, hi_x) = if a1.x < a2.x { (a1.x, a2.x) } else { (a2.x, a1.x) };
        let (lo_y, hi_y) = if a1.y < a2.y { (a1.y, a2.y) } else { (a2.y, a1.y) };
        let (lo_x2, hi_x2) = if b1.x < b2.x { (b1.x, b2.x) } else { (b2.x, b1.x) };
        let (lo_y2, hi_y2) = if b1.y < b2.y { (b1.y, b2.y) } else { (b2.y, b1.y) };
        if hi_x < lo_x2 - eps
            || lo_x > hi_x2 + eps
            || hi_y < lo_y2 - eps
            || lo_y > hi_y2 + eps
        {
            return false;
        }
    }
    let dx_a = a2.x - a1.x;
    let dy_a = a2.y - a1.y;
    let dx_b = b2.x - b1.x;
    let dy_b = b2.y - b1.y;
    let f1 = dx_a * (b1.y - a1.y) - dy_a * (b1.x - a1.x);
    let f2 = dx_a * (b2.y - a1.y) - dy_a * (b2.x - a1.x);
    let f3 = dx_b * (a1.y - b1.y) - dy_b * (a1.x - b1.x);
    let f4 = dx_b * (a2.y - b1.y) - dy_b * (a2.x - b1.x);
    #[inline(always)]
    fn orient_err(t1: f64, t2: f64) -> f64 {
        32.0 * f64::EPSILON * (t1.abs() + t2.abs())
    }
    let e1 = orient_err(dx_a * (b1.y - a1.y), dy_a * (b1.x - a1.x));
    let e2 = orient_err(dx_a * (b2.y - a1.y), dy_a * (b2.x - a1.x));
    let e3 = orient_err(dx_b * (a1.y - b1.y), dy_b * (a1.x - b1.x));
    let e4 = orient_err(dx_b * (a2.y - b1.y), dy_b * (a2.x - b1.x));
    // Vertex-on-edge gates: |orient| <= 1e-12 * (tested segment len2).
    let voe_a = 1e-12 * (dx_a * dx_a + dy_a * dy_a);
    let voe_b = 1e-12 * (dx_b * dx_b + dy_b * dy_b);
    if f1.abs() > e1.max(voe_a) + e1
        && f2.abs() > e2.max(voe_a) + e2
        && f3.abs() > e3.max(voe_b) + e3
        && f4.abs() > e4.max(voe_b) + e4
    {
        // Zero-safe strict opposite sign (matches the exact path).
        return (f1 > 0.0 && f2 < 0.0 || f1 < 0.0 && f2 > 0.0)
            && (f3 > 0.0 && f4 < 0.0 || f3 < 0.0 && f4 > 0.0);
    }
    *ambiguous = true;
    edges_intersect_general(a1, a2, b1, b2, eps) || edges_vertex_on_edge(a1, a2, b1, b2)
}

pub(crate) fn check_edge_pair_intersection(
    coords: &[Coord<f64>],
    i: usize,
    j: usize,
    eps: f64,
) -> bool {
    let n = coords.len() - 1;
    let a1 = coords[i];
    // min(n) instead of % n: ring slices close (coords[n] == coords[0]),
    // so both are identical for rings; OPEN line slices must see the real
    // last coordinate (the % n wrap turns segment n-1 into a phantom
    // crossing chord - measured 2026-08-07, sine-wave lines flagged
    // non-simple).
    let a2 = coords[(i + 1).min(n)];
    let b1 = coords[j];
    let b2 = coords[(j + 1).min(n)];
    // Ring path: the lean predicate. No vertex-revisit class here - shared
    // vertices between non-adjacent ring edges are the pinch class, owned
    // by the caller's classification (check_ring_validity), and must not
    // short-circuit the pair test. The T-junction class (vertex strictly on
    // a non-adjacent edge, GEOS Test 22) escalates through the lean's
    // vertex-on-edge gate.
    let mut ambiguous = false;
    lean_pair_intersects(a1, a2, b1, b2, eps, &mut ambiguous)
}

/// Lean vertex-on-edge prefilter: true only when some endpoint's fast
/// orient sits within the VOE threshold of the tested segment
/// (`1e-12 * len2` + the orient error bound). Decision-equivalent to
/// `edges_vertex_on_edge` - a pair with every orient beyond the bound
/// cannot have a vertex on the other segment. The structure gate's pair
/// sweep pays 4 crosses + 4 compares instead of the full
/// point_strictly_on_segment chain per same-ring pair (measured
/// 2026-08-07: the chain is ~40-80 ns/pair on clean data).
#[inline]
pub(crate) fn lean_voe_possible(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
) -> bool {
    let dx_a = a2.x - a1.x;
    let dy_a = a2.y - a1.y;
    let dx_b = b2.x - b1.x;
    let dy_b = b2.y - b1.y;
    let f1 = dx_a * (b1.y - a1.y) - dy_a * (b1.x - a1.x);
    let f2 = dx_a * (b2.y - a1.y) - dy_a * (b2.x - a1.x);
    let f3 = dx_b * (a1.y - b1.y) - dy_b * (a1.x - b1.x);
    let f4 = dx_b * (a2.y - b1.y) - dy_b * (a2.x - b1.x);
    let voe_a = 1e-12 * (dx_a * dx_a + dy_a * dy_a);
    let voe_b = 1e-12 * (dx_b * dx_b + dy_b * dy_b);
    let e1 = 32.0 * f64::EPSILON * ((dx_a * (b1.y - a1.y)).abs() + (dy_a * (b1.x - a1.x)).abs());
    let e2 = 32.0 * f64::EPSILON * ((dx_a * (b2.y - a1.y)).abs() + (dy_a * (b2.x - a1.x)).abs());
    let e3 = 32.0 * f64::EPSILON * ((dx_b * (a1.y - b1.y)).abs() + (dy_b * (a1.x - b1.x)).abs());
    let e4 = 32.0 * f64::EPSILON * ((dx_b * (a2.y - b1.y)).abs() + (dy_b * (a2.x - b1.x)).abs());
    f1.abs() <= voe_a + e1
        || f2.abs() <= voe_a + e2
        || f3.abs() <= voe_b + e3
        || f4.abs() <= voe_b + e4
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
pub(crate) fn point_strictly_on_segment(p: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> bool {
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
pub(crate) struct EdgeIdx {
    pub(crate) idx: usize,
    pub(crate) env: rstar::AABB<[f64; 2]>,
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
pub(crate) fn build_ring_edge_tree(ring: &[Coord<f64>]) -> rstar::RTree<EdgeIdx> {
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
pub(crate) fn build_ls_edge_tree(coords: &[Coord<f64>]) -> rstar::RTree<EdgeIdx> {
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
                    core::ops::ControlFlow::Break(())
                } else {
                    core::ops::ControlFlow::<(), ()>::Continue(())
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

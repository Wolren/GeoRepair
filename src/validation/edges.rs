//! edge-pair intersection predicates and ring edge trees
//!
//! Extracted from validation/core.rs on 2026-08-07 (file-size governance: core.rs was
//! 2540 lines; the cap is 800). Content is verbatim - no behavior changes; sibling
//! modules resolve shared items through the `crate::validation::core` facade.
//!
//! See validation/mod.rs for the module map.

#[cfg(feature = "rstar")]
use alloc::vec::Vec;
use geo::Coord;

pub(crate) fn edges_intersect_general(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
) -> bool {
    // up to eps (e.g. a 5e-14-tall sliver over a length-10 base, eps 1e-11 -
    // raw-disjoint y-ranges, genuinely within the gate;)
    {
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
        let (lo_x2, hi_x2) = if b1.x < b2.x {
            (b1.x, b2.x)
        } else {
            (b2.x, b1.x)
        };
        let (lo_y2, hi_y2) = if b1.y < b2.y {
            (b1.y, b2.y)
        } else {
            (b2.y, b1.y)
        };
        if hi_x < lo_x2 - eps || lo_x > hi_x2 + eps || hi_y < lo_y2 - eps || lo_y > hi_y2 + eps {
            return false;
        }
    }
    // exact (the error is ~4 ulps of L2; the gate is 32 ulps)
    let dx_a = a2.x - a1.x;
    let dy_a = a2.y - a1.y;
    let dx_b = b2.x - b1.x;
    let dy_b = b2.y - b1.y;
    let f1 = dx_a * (b1.y - a1.y) - dy_a * (b1.x - a1.x);
    let f2 = dx_a * (b2.y - a1.y) - dy_a * (b2.x - a1.x);
    let f3 = dx_b * (a1.y - b1.y) - dy_b * (a1.x - b1.x);
    let f4 = dx_b * (a2.y - b1.y) - dy_b * (a2.x - b1.x);
    // `32 * epsilon * max(la2, lb2)` margin used the edge lengths The product-sum bound
    // for the same pair is ~1e-20 - the orient is definitively not collinear.
    #[inline(always)]
    fn orient_err(t1: f64, t2: f64) -> f64 {
        32.0 * f64::EPSILON * (t1.abs() + t2.abs())
    }
    let e1 = orient_err(dx_a * (b1.y - a1.y), dy_a * (b1.x - a1.x));
    let e2 = orient_err(dx_a * (b2.y - a1.y), dy_a * (b2.x - a1.x));
    let e3 = orient_err(dx_b * (a1.y - b1.y), dy_b * (a1.x - b1.x));
    let e4 = orient_err(dx_b * (a2.y - b1.y), dy_b * (a2.x - b1.x));
    if f1.abs() > 2.0 * e1 && f2.abs() > 2.0 * e2 && f3.abs() > 2.0 * e3 && f4.abs() > 2.0 * e4 {
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

    // collinear touch as a crossing (false SelfIntersection on valid geometry with -0.0
    // coordinates; measured: this inflated the real-world "2,298 invalid")
    if (o1 > 0.0 && o2 < 0.0 || o1 < 0.0 && o2 > 0.0)
        && (o3 > 0.0 && o4 < 0.0 || o3 < 0.0 && o4 > 0.0)
    {
        return true;
    }

    // separated near-parallel edges (measured: invariant_sliver_hole seed cc 9b38e427,
    // scale=51.29, sliver_width=1e-12: parallel edges 1e-12 apart gave exact orients
    // 2.05e-11 <) (same rationale as the fast-first gate above - the L2-based margin
    // inflates past nearly-parallel pairs when one edge is long).
    let collinear = o1.abs() <= e1 && o2.abs() <= e2;
    if collinear {
        let dx = a2.x - a1.x;
        let dy = a2.y - a1.y;
        let len2 = dx * dx + dy * dy;
        if len2 > 0.0 {
            let t1 = ((b1.x - a1.x) * dx + (b1.y - a1.y) * dy) / len2;
            let t2 = ((b2.x - a1.x) * dx + (b2.y - a1.y) * dy) / len2;
            let lo = 0.0f64.max(t1.min(t2));
            let hi = 1.0f64.min(t1.max(t2));
            let overlap_a = hi - lo;
            if o1 == 0.0 && o2 == 0.0 {
                // (fuzz-nightly class C, run 35578735657 on 2026-09-21: the CW ring
                // passed the gate and the validator, the re-wound ring failed)
                if overlap_a > 0.0 && (overlap_a * len2.sqrt()).is_normal() {
                    return true;
                }
                let bx = b2.x - b1.x;
                let by = b2.y - b1.y;
                let blen2 = bx * bx + by * by;
                if blen2 > 0.0 {
                    let s1 = ((a1.x - b1.x) * bx + (a1.y - b1.y) * by) / blen2;
                    let s2 = ((a2.x - b1.x) * bx + (a2.y - b1.y) * by) / blen2;
                    let lo_b = 0.0f64.max(s1.min(s2));
                    let hi_b = 1.0f64.min(s1.max(s2));
                    if hi_b - lo_b > 0.0 && ((hi_b - lo_b) * blen2.sqrt()).is_normal() {
                        return true;
                    }
                }
            } else if len2 > eps && overlap_a > eps {
                return true;
            }
        }
    }

    false
}

/// Lean pair intersection, decision-equivalent to edges_intersect_general or
/// edges_vertex_on_edge; strong pairs skip that chain. The gate is 1e-12 times tested
/// len2, plus the error margin. The voe gate is 140x the margin, so a voe-range
/// endpoint sits 70x beyond (2026-08-07). The strong test must cover both bounds, or
/// lean misses vertex-on-edge. All 120k star-comb pairs are strong.
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
    {
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
        let (lo_x2, hi_x2) = if b1.x < b2.x {
            (b1.x, b2.x)
        } else {
            (b2.x, b1.x)
        };
        let (lo_y2, hi_y2) = if b1.y < b2.y {
            (b1.y, b2.y)
        } else {
            (b2.y, b1.y)
        };
        if hi_x < lo_x2 - eps || lo_x > hi_x2 + eps || hi_y < lo_y2 - eps || lo_y > hi_y2 + eps {
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
    // last coordinate (the % n wrap turns segment n-1 into a phantom crossing chord -
    // measured 2026-08-07, sine-wave lines flagged)
    let a2 = coords[(i + 1).min(n)];
    let b1 = coords[j];
    let b2 = coords[(j + 1).min(n)];
    // The T-junction class (vertex strictly on a non-adjacent edge, GEOS Test 22)
    // escalates through the lean's vertex-on-edge gate.
    let mut ambiguous = false;
    lean_pair_intersects(a1, a2, b1, b2, eps, &mut ambiguous)
}

/// Lean vertex-on-edge prefilter. It returns true only when an endpoint fast orient
/// sits within the voe threshold: 1e-12 * len2 plus the orient bound. That chain costs
/// 40-80 ns per pair on clean data (2026-08-07).
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

/// Strict-interior vertex-on-edge touch: an endpoint of one segment lies strictly
/// inside the other. This runs in the per-pair small-ring sweep on the 1.58M-poly hot
/// path, and a non-inlined call cost +25 % overall. The tolerance is segment-local,
/// 1e-12 * len2 of the tested segment. A pair-max tolerance inflates past micro
/// segments in mixed rings. Differential fuzz (2026-08-03, seed 85) shows the class.
#[inline]
pub(crate) fn edges_vertex_on_edge(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
) -> bool {
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
    let (lo_x2, hi_x2) = if b1.x < b2.x {
        (b1.x, b2.x)
    } else {
        (b2.x, b1.x)
    };
    let (lo_y2, hi_y2) = if b1.y < b2.y {
        (b1.y, b2.y)
    } else {
        (b2.y, b1.y)
    };
    if hi_x < lo_x2 || lo_x > hi_x2 || hi_y < lo_y2 || lo_y > hi_y2 {
        return false;
    }
    point_strictly_on_segment(a1, b1, b2)
        || point_strictly_on_segment(a2, b1, b2)
        || point_strictly_on_segment(b1, a1, a2)
        || point_strictly_on_segment(b2, a1, a2)
}

/// True if p lies strictly inside segment (a, b): within eps of its line, between the
/// endpoints. Endpoint equality returns false. Measured: a 3e-8 closing edge crossed by
/// a vertex of a 2.3e6-scale edge. The pair-max eps, 1e-12 * 1.8e13 or about 18, made
/// the test vacuous. geos flagged Ring Self-intersection[1e-08 -1e-08]; we accepted it
/// (2026-08-03).
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
pub(crate) fn check_rings_intersect(ring1: &[Coord<f64>], ring2: &[Coord<f64>], eps: f64) -> bool {
    let n1 = ring1.len().max(2) - 1;
    let n2 = ring2.len().max(2) - 1;
    if n1 < 2 || n2 < 2 {
        return false;
    }

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

#![allow(dead_code)]
use crate::orient::orient2d_fast;
use alloc::vec::Vec;
use core::mem;
use geo::{Coord, GeoFloat, Line};

// ── Sealed trait: safe f64 downcast ────────────────────────────────────
//
// Only f64 and f32 implement NodingFloat.  Inside the f64‑only helpers no
// unsafe code appears — safe `.to_f64()` conversions are used at the
// dispatch boundary.

mod private {
    use geo::{Coord, GeoFloat};

    /// Trait for numeric types usable in noding algorithms.
    ///
    /// Extends [`GeoFloat`] with a deterministic coordinate hashing method
    /// used for HashMap-based deduplication during noding.
    pub trait NodingFloat: GeoFloat {
        /// Deterministic hash from coordinate bits for HashMap keys.
        fn coord_hash_key(c: &Coord<Self>) -> u64;
    }

    impl NodingFloat for f64 {
        fn coord_hash_key(c: &Coord<f64>) -> u64 {
            c.x.to_bits() ^ c.y.to_bits().rotate_left(32)
        }
    }

    impl NodingFloat for f32 {
        fn coord_hash_key(c: &Coord<f32>) -> u64 {
            (c.x.to_bits() as u64) ^ (c.y.to_bits() as u64).rotate_left(32)
        }
    }
}

pub(crate) use private::NodingFloat;

/// Returns true when every edge shares one of two common endpoints —
/// a radial/star topology that cannot produce proper crossings.
fn is_radial<T: GeoFloat>(edges: &[Line<T>]) -> bool {
    if edges.len() < 3 {
        return false;
    }
    let a = edges[0].start;
    let b = edges[0].end;
    if a == b {
        return false;
    }
    edges
        .iter()
        .all(|e| e.start == a || e.end == a || e.start == b || e.end == b)
}

pub(crate) fn check_self_intersections<T: GeoFloat>(edges: &[Line<T>]) -> bool {
    let eps = T::from(1e-12).expect("1e-12 fits any GeoFloat");
    if edges.len() < 3 {
        return false;
    }

    // Radial geometry fast path: all edges share a common endpoint so
    // no non-adjacent pair can properly cross — only touch at that point.
    if is_radial(edges) {
        return false;
    }

    // R‑tree path for large edge sets (safe conversion, no transmute)
    #[cfg(feature = "rstar")]
    if edges.len() >= 64 {
        let edges_f64: Vec<Line<f64>> = edges
            .iter()
            .map(|l| {
                Line::new(
                    Coord {
                        x: l.start.x.to_f64().expect("to_f64"),
                        y: l.start.y.to_f64().expect("to_f64"),
                    },
                    Coord {
                        x: l.end.x.to_f64().expect("to_f64"),
                        y: l.end.y.to_f64().expect("to_f64"),
                    },
                )
            })
            .collect();
        return check_self_intersections_f64(&edges_f64, 1e-12);
    }

    // Brute force for smaller edge sets or when rstar unavailable
    for i in 0..edges.len() {
        for j in (i + 2)..edges.len() {
            let e1 = &edges[i];
            let e2 = &edges[j];
            if e1.start == e2.start && orient2d_generic(e1.start, e1.end, e2.end) != T::zero() {
                continue;
            }
            if e1.start == e2.end && orient2d_generic(e1.start, e1.end, e2.start) != T::zero() {
                continue;
            }
            if e1.end == e2.start && orient2d_generic(e1.end, e1.start, e2.end) != T::zero() {
                continue;
            }
            if e1.end == e2.end && orient2d_generic(e1.end, e1.start, e2.start) != T::zero() {
                continue;
            }
            if edges_intersect(&edges[i], &edges[j], eps) {
                return true;
            }
        }
    }
    false
}

#[cfg(feature = "rstar")]
fn check_self_intersections_f64(edges: &[Line<f64>], eps: f64) -> bool {
    let n = edges.len();
    if n < 3 {
        return false;
    }

    #[derive(Clone, Copy)]
    struct EdgeEnv {
        idx: usize,
        env: rstar::AABB<[f64; 2]>,
    }
    impl rstar::RTreeObject for EdgeEnv {
        type Envelope = rstar::AABB<[f64; 2]>;
        fn envelope(&self) -> Self::Envelope {
            self.env
        }
    }

    let envs: Vec<EdgeEnv> = edges
        .iter()
        .enumerate()
        .map(|(i, e)| EdgeEnv {
            idx: i,
            env: rstar::AABB::from_corners(
                [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
                [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
            ),
        })
        .collect();
    let tree = rstar::RTree::bulk_load(envs);

    for i in 0..n {
        let e = &edges[i];
        let query = rstar::AABB::from_corners(
            [e.start.x.min(e.end.x), e.start.y.min(e.end.y)],
            [e.start.x.max(e.end.x), e.start.y.max(e.end.y)],
        );
        let result = tree.locate_in_envelope_intersecting_int(query, |c| {
            let j = c.idx;
            if j <= i {
                return core::ops::ControlFlow::Continue(());
            }
            if i + 1 == j && edges[i].end == edges[j].start {
                return core::ops::ControlFlow::Continue(());
            }
            if j + 1 == i && edges[j].end == edges[i].start {
                return core::ops::ControlFlow::Continue(());
            }
            if edges[i].start == edges[j].start
                && orient2d_fast(edges[i].start, edges[i].end, edges[j].end) != 0.0
            {
                return core::ops::ControlFlow::Continue(());
            }
            if edges[i].start == edges[j].end
                && orient2d_fast(edges[i].start, edges[i].end, edges[j].start) != 0.0
            {
                return core::ops::ControlFlow::Continue(());
            }
            if edges[i].end == edges[j].start
                && orient2d_fast(edges[i].end, edges[i].start, edges[j].end) != 0.0
            {
                return core::ops::ControlFlow::Continue(());
            }
            if edges[i].end == edges[j].end
                && orient2d_fast(edges[i].end, edges[i].start, edges[j].start) != 0.0
            {
                return core::ops::ControlFlow::Continue(());
            }
            if edges_intersect(&edges[i], &edges[j], eps) {
                core::ops::ControlFlow::Break(())
            } else {
                core::ops::ControlFlow::Continue(())
            }
        });
        if result.is_break() {
            return true;
        }
    }
    false
}

pub(crate) fn edges_intersect<T: GeoFloat>(e1: &Line<T>, e2: &Line<T>, eps: T) -> bool {
    if e1.start == e2.start && orient2d_generic(e1.start, e1.end, e2.end) != T::zero() {
        return false;
    }
    if e1.start == e2.end && orient2d_generic(e1.start, e1.end, e2.start) != T::zero() {
        return false;
    }
    if e1.end == e2.start && orient2d_generic(e1.end, e1.start, e2.end) != T::zero() {
        return false;
    }
    if e1.end == e2.end && orient2d_generic(e1.end, e1.start, e2.start) != T::zero() {
        return false;
    }
    // For f64, use robust orient2d (Shewchuk's algorithm)
    // to avoid false negatives with near-collinear edges at large coordinates.
    if mem::size_of::<T>() == 8 {
        return edges_intersect_f64_robust(
            &Line::new(
                Coord {
                    x: e1.start.x.to_f64().expect("to_f64"),
                    y: e1.start.y.to_f64().expect("to_f64"),
                },
                Coord {
                    x: e1.end.x.to_f64().expect("to_f64"),
                    y: e1.end.y.to_f64().expect("to_f64"),
                },
            ),
            &Line::new(
                Coord {
                    x: e2.start.x.to_f64().expect("to_f64"),
                    y: e2.start.y.to_f64().expect("to_f64"),
                },
                Coord {
                    x: e2.end.x.to_f64().expect("to_f64"),
                    y: e2.end.y.to_f64().expect("to_f64"),
                },
            ),
            eps.to_f64().expect("to_f64"),
        );
    }
    let o1 = orient2d_generic(e1.start, e1.end, e2.start);
    let o2 = orient2d_generic(e1.start, e1.end, e2.end);
    let o3 = orient2d_generic(e2.start, e2.end, e1.start);
    let o4 = orient2d_generic(e2.start, e2.end, e1.end);

    // General case (proper crossing)
    if o1.abs() > eps && o2.abs() > eps && o3.abs() > eps && o4.abs() > eps {
        return o1.signum() != o2.signum() && o3.signum() != o4.signum();
    }

    // Collinear case: check for interval overlap along the shared line
    if o1.abs() <= eps && o2.abs() <= eps && o3.abs() <= eps && o4.abs() <= eps {
        return collinear_overlap(e1, e2, eps);
    }

    // Endpoint touching — not considered an intersection for noding purposes
    false
}

/// Robust intersection test for f64 edges using Shewchuk's orient2d.
fn edges_intersect_f64_robust(e1: &Line<f64>, e2: &Line<f64>, eps: f64) -> bool {
    if e1.start == e2.start && orient2d_fast(e1.start, e1.end, e2.end) != 0.0 {
        return false;
    }
    if e1.start == e2.end && orient2d_fast(e1.start, e1.end, e2.start) != 0.0 {
        return false;
    }
    if e1.end == e2.start && orient2d_fast(e1.end, e1.start, e2.end) != 0.0 {
        return false;
    }
    if e1.end == e2.end && orient2d_fast(e1.end, e1.start, e2.start) != 0.0 {
        return false;
    }
    let batch = crate::simd::orient2d_batch_4_robust(
        &[e1.start, e1.start, e2.start, e2.start],
        &[e1.end, e1.end, e2.end, e2.end],
        &[e2.start, e2.end, e1.start, e1.end],
    );
    let (o1, o2, o3, o4) = (batch[0], batch[1], batch[2], batch[3]);

    if o1.abs() > eps && o2.abs() > eps && o3.abs() > eps && o4.abs() > eps {
        return o1.signum() != o2.signum() && o3.signum() != o4.signum();
    }

    if o1.abs() <= eps && o2.abs() <= eps && o3.abs() <= eps && o4.abs() <= eps {
        return collinear_overlap(e1, e2, eps);
    }

    false
}

/// Check whether two collinear segments overlap along the shared line.
fn collinear_overlap<T: GeoFloat>(e1: &Line<T>, e2: &Line<T>, eps: T) -> bool {
    let dx = e1.end.x - e1.start.x;
    let dy = e1.end.y - e1.start.y;
    let dot_d = dx * dx + dy * dy;
    if dot_d <= eps {
        return false;
    }

    // Project e2 endpoints onto e1's direction (t parameter along e1)
    let t2s = ((e2.start.x - e1.start.x) * dx + (e2.start.y - e1.start.y) * dy) / dot_d;
    let t2e = ((e2.end.x - e1.start.x) * dx + (e2.end.y - e1.start.y) * dy) / dot_d;

    let (t2_min, t2_max) = if t2s < t2e { (t2s, t2e) } else { (t2e, t2s) };

    // Overlap region on [0, 1]
    let lo = T::zero().max(t2_min);
    let hi = T::one().min(t2_max);

    // True if overlap covers more than epsilon-length
    lo + eps < hi
}

pub(crate) fn orient2d_generic<T: GeoFloat>(a: Coord<T>, b: Coord<T>, c: Coord<T>) -> T {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

pub(crate) fn interpolate<T: GeoFloat>(e: Line<T>, t: T) -> Coord<T> {
    Coord {
        x: e.start.x + t * (e.end.x - e.start.x),
        y: e.start.y + t * (e.end.y - e.start.y),
    }
}

pub(crate) fn dist2<T: GeoFloat>(a: Coord<T>, b: Coord<T>) -> T {
    (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
}

/// For two collinear segments, compute split t-parameters where one edge's
/// endpoint falls strictly inside the other. Returns `(splits_on_e1, splits_on_e2)`.
pub(crate) fn collinear_split_params<T: GeoFloat>(
    e1: &Line<T>,
    e2: &Line<T>,
    eps: T,
) -> (Vec<T>, Vec<T>) {
    let eps_sq = eps * eps;

    // Project e2 endpoints onto e1's direction
    let dx1 = e1.end.x - e1.start.x;
    let dy1 = e1.end.y - e1.start.y;
    let dot1 = dx1 * dx1 + dy1 * dy1;
    let (mut p1, mut p2) = (Vec::new(), Vec::new());
    if dot1 > eps_sq {
        let t2s = ((e2.start.x - e1.start.x) * dx1 + (e2.start.y - e1.start.y) * dy1) / dot1;
        let t2e = ((e2.end.x - e1.start.x) * dx1 + (e2.end.y - e1.start.y) * dy1) / dot1;
        let one = T::one();
        if t2s > eps && t2s < one - eps {
            p1.push(t2s);
        }
        if t2e > eps && t2e < one - eps {
            p1.push(t2e);
        }
    }

    // Project e1 endpoints onto e2's direction
    let dx2 = e2.end.x - e2.start.x;
    let dy2 = e2.end.y - e2.start.y;
    let dot2 = dx2 * dx2 + dy2 * dy2;
    if dot2 > eps_sq {
        let t1s = ((e1.start.x - e2.start.x) * dx2 + (e1.start.y - e2.start.y) * dy2) / dot2;
        let t1e = ((e1.end.x - e2.start.x) * dx2 + (e1.end.y - e2.start.y) * dy2) / dot2;
        let one = T::one();
        if t1s > eps && t1s < one - eps {
            p2.push(t1s);
        }
        if t1e > eps && t1e < one - eps {
            p2.push(t1e);
        }
    }

    (p1, p2)
}

/// f64‑only DD‑based intersection for maximum robustness.
/// Callers MUST still check t/u in the desired range (this function
/// returns the intersection even when the crossing falls outside segments).
fn compute_intersection_param_dd(e1: &Line<f64>, e2: &Line<f64>) -> Option<(f64, f64, Coord<f64>)> {
    crate::dd::segment_intersection_dd(e1.start, e1.end, e2.start, e2.end)
        .map(|(pt, t, u)| (t.to_f64(), u.to_f64(), pt))
}

pub(crate) fn compute_intersection_param<T: GeoFloat>(
    e1: &Line<T>,
    e2: &Line<T>,
    eps: T,
) -> Option<(T, T, Coord<T>)> {
    // For f64: GEOS-style two-phase — orient2d detection then DD computation.
    if core::mem::size_of::<T>() == 8 {
        let ef1 = Line::new(
            Coord {
                x: e1.start.x.to_f64().expect("to_f64"),
                y: e1.start.y.to_f64().expect("to_f64"),
            },
            Coord {
                x: e1.end.x.to_f64().expect("to_f64"),
                y: e1.end.y.to_f64().expect("to_f64"),
            },
        );
        let ef2 = Line::new(
            Coord {
                x: e2.start.x.to_f64().expect("to_f64"),
                y: e2.start.y.to_f64().expect("to_f64"),
            },
            Coord {
                x: e2.end.x.to_f64().expect("to_f64"),
                y: e2.end.y.to_f64().expect("to_f64"),
            },
        );
        // Phase 1: Detection via batch robust orient2d (SIMD fast path +
        // Shewchuk adaptive-precision fallback for near-zero results).
        let batch = crate::simd::orient2d_batch_4_robust(
            &[ef1.start, ef1.start, ef2.start, ef2.start],
            &[ef1.end, ef1.end, ef2.end, ef2.end],
            &[ef2.start, ef2.end, ef1.start, ef1.end],
        );
        let (o1, o2, o3, o4) = (batch[0], batch[1], batch[2], batch[3]);

        // Quick rejection: both endpoints of one segment on the same side
        // of the other segment → they cannot intersect (fast path).
        if o1.signum() == o2.signum() && o1 != 0.0 && o2 != 0.0 {
            return None;
        }
        if o3.signum() == o4.signum() && o3 != 0.0 && o4 != 0.0 {
            return None;
        }

        // Phase 2: Computation via DD (106-bit mantissa) for any
        // non-trivially-separated case: proper crossing, endpoint-on-
        // segment, or near-parallel.  Let caller handle collinear
        // overlap via `None`.
        return compute_intersection_param_dd(&ef1, &ef2).map(|(t, u, pt)| {
            (
                T::from(t).expect("intersection param in range"),
                T::from(u).expect("intersection param in range"),
                Coord {
                    x: T::from(pt.x).expect("intersection coord in range"),
                    y: T::from(pt.y).expect("intersection coord in range"),
                },
            )
        });
    }

    // Generic fallback for non-f64 types (f32, etc.)
    let denom = (e1.end.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e1.end.y - e1.start.y) * (e2.end.x - e2.start.x);
    if denom.abs() < eps {
        return None;
    }
    let t = ((e2.start.x - e1.start.x) * (e2.end.y - e2.start.y)
        - (e2.start.y - e1.start.y) * (e2.end.x - e2.start.x))
        / denom;
    let u = ((e2.start.x - e1.start.x) * (e1.end.y - e1.start.y)
        - (e2.start.y - e1.start.y) * (e1.end.x - e1.start.x))
        / denom;
    let pt = Coord {
        x: e1.start.x + t * (e1.end.x - e1.start.x),
        y: e1.start.y + t * (e1.end.y - e1.start.y),
    };
    Some((t, u, pt))
}

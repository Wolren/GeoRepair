//! Per-pair classification for the lean line noder: mirrors the validator's own
//! predicate chain (fast-FP first, robust escalation, collinear, vertex-on-edge, shared

use geo::Coord;

use crate::orient::orient2d;

use super::Hit;

/// Classify a segment pair with the validator's own predicate semantics.
#[inline]
pub(super) fn classify(
    a1: Coord<f64>,
    a2: Coord<f64>,
    b1: Coord<f64>,
    b2: Coord<f64>,
    eps: f64,
) -> Hit {
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
    // validator's eps sweep rejects (fuzz crash-9c50dad6: vertex 4.6e-311 off the far
    // segment's line, same side as partner, vs eps 5.26e71;)
    let contact = |p: Coord<f64>, s1: Coord<f64>, s2: Coord<f64>, f: f64, e: f64| {
        (p != s1 && p != s2)
            && f.abs() <= eps + e
            && p.x >= s1.x.min(s2.x) - eps
            && p.x <= s1.x.max(s2.x) + eps
            && p.y >= s1.y.min(s2.y) - eps
            && p.y <= s1.y.max(s2.y) + eps
            && (p.x - s1.x).abs().max((p.y - s1.y).abs()) > eps
            && (p.x - s2.x).abs().max((p.y - s2.y).abs()) > eps
    };
    let vertex_contact = || -> Option<Coord<f64>> {
        if contact(a1, b1, b2, f3, e3) {
            return Some(a1);
        }
        if contact(a2, b1, b2, f4, e4) {
            return Some(a2);
        }
        if contact(b1, a1, a2, f1, e1) {
            return Some(b1);
        }
        if contact(b2, a1, a2, f2, e2) {
            return Some(b2);
        }
        None
    };
    let definitive =
        f1.abs() > 2.0 * e1 && f2.abs() > 2.0 * e2 && f3.abs() > 2.0 * e3 && f4.abs() > 2.0 * e4;
    let opposite = (f1 > 0.0 && f2 < 0.0 || f1 < 0.0 && f2 > 0.0)
        && (f3 > 0.0 && f4 < 0.0 || f3 < 0.0 && f4 > 0.0);
    if definitive {
        if opposite {
            return cross_point(a1, a2, b1, b2);
        }
        if let Some(v) = vertex_contact() {
            return Hit::VertexOnEdge(v);
        }
        return Hit::None;
    }
    let o1 = orient2d(a1, a2, b1);
    let o2 = orient2d(a1, a2, b2);
    let o3 = orient2d(b1, b2, a1);
    let o4 = orient2d(b1, b2, a2);
    // 32-ulp band are FP-ambiguous (near-coincident lines, e.g. a segment pair whose
    // endpoints coincide to 1 ulp from the lissajous retrace)
    if (o1.abs() > e1 || o2.abs() > e2 || o3.abs() > e3 || o4.abs() > e4)
        && (o1 > 0.0 && o2 < 0.0 || o1 < 0.0 && o2 > 0.0)
        && (o3 > 0.0 && o4 < 0.0 || o3 < 0.0 && o4 > 0.0)
    {
        return cross_point(a1, a2, b1, b2);
    }
    if o1.abs() <= e1 && o2.abs() <= e2 {
        let len2 = dx_a * dx_a + dy_a * dy_a;
        if len2 > eps {
            let t1 = ((b1.x - a1.x) * dx_a + (b1.y - a1.y) * dy_a) / len2;
            let t2 = ((b2.x - a1.x) * dx_a + (b2.y - a1.y) * dy_a) / len2;
            let lo = 0.0f64.max(t1.min(t2));
            let hi = 1.0f64.min(t1.max(t2));
            if hi - lo > eps {
                return Hit::Collinear;
            }
        } else if len2 > 0.0 && o1 == 0.0 && o2 == 0.0 {
            let t1 = ((b1.x - a1.x) * dx_a + (b1.y - a1.y) * dy_a) / len2;
            let t2 = ((b2.x - a1.x) * dx_a + (b2.y - a1.y) * dy_a) / len2;
            let lo = 0.0f64.max(t1.min(t2));
            let hi = 1.0f64.min(t1.max(t2));
            if hi - lo > 0.0 {
                return Hit::Collinear;
            }
        }
    }
    if let Some(v) = vertex_contact() {
        return Hit::VertexOnEdge(v);
    }
    if crate::validation::edges::point_strictly_on_segment(a1, b1, b2) {
        return Hit::VertexOnEdge(a1);
    }
    if crate::validation::edges::point_strictly_on_segment(a2, b1, b2) {
        return Hit::VertexOnEdge(a2);
    }
    if crate::validation::edges::point_strictly_on_segment(b1, a1, a2) {
        return Hit::VertexOnEdge(b1);
    }
    if crate::validation::edges::point_strictly_on_segment(b2, a1, a2) {
        return Hit::VertexOnEdge(b2);
    }
    if a1 == b1 || a1 == b2 || a2 == b1 || a2 == b2 {
        return Hit::Shared;
    }
    Hit::None
}

#[inline]
fn cross_point(a1: Coord<f64>, a2: Coord<f64>, b1: Coord<f64>, b2: Coord<f64>) -> Hit {
    match crate::dd::segment_intersection_dd(a1, a2, b1, b2) {
        Some((pt, _, _)) if !pt.x.is_nan() && !pt.y.is_nan() => Hit::Cross(pt),
        // A NaN node would poison the cluster sort.
        _ => Hit::Collinear,
    }
}

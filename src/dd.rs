//! Double-double arithmetic for robust geometric constructions.
//!
//! Each value is stored as a pair (hi, lo) where the real value is `hi + lo`.
//! This gives ~106 bits of mantissa (roughly 2× f64 precision, ~31 decimal digits).
//!
//! Based on the algorithms of Dekker, Knuth, and Ogita–Rump–Oishi.
//! Used by GEOS and JTS for exact line intersection and coordinate computation.
//!
//! # Profiling
//! `dd_call_count` and `reset_dd_count` track total invocations of
//! `segment_intersection_dd` across the repair pipeline.

use core::sync::atomic::{AtomicU64, Ordering};

pub(crate) static DD_CALL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Reset the DD call counter to zero.  Call before a profiling run.
pub fn reset_dd_count() {
    DD_CALL_COUNT.store(0, Ordering::Relaxed);
}

/// Return the total number of `segment_intersection_dd` calls since the
/// last reset (or program start).
pub fn dd_call_count() -> u64 {
    DD_CALL_COUNT.load(Ordering::Relaxed)
}

use geo::Coord;

/// A double-double number: hi + lo, where |lo| ≤ 0.5 ulp(hi).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DD {
    /// High (most significant) part of the double-double value.
    pub hi: f64,
    /// Low (least significant) part of the double-double value, |lo| ≤ 0.5 ulp(hi).
    pub lo: f64,
}

impl DD {
    /// Create a DD from explicit hi and lo parts.
    #[inline]
    #[allow(dead_code)]
    pub fn new(hi: f64, lo: f64) -> Self {
        DD { hi, lo }
    }

    /// Create a DD from a single f64 (lo = 0).
    #[inline]
    pub fn from_f64(x: f64) -> Self {
        DD { hi: x, lo: 0.0 }
    }

    /// Sum of two numbers with error term (Knuth's two-sum).
    #[inline]
    fn two_sum(a: f64, b: f64) -> (f64, f64) {
        let x = a + b;
        let bv = x - a;
        let av = x - bv;
        let br = b - bv;
        let ar = a - av;
        (x, ar + br)
    }

    /// Split a f64 into two parts, each with 26 bits of mantissa (Dekker).
    #[inline]
    fn split(a: f64) -> (f64, f64) {
        let splitter = (1u64 << 27) as f64 + 1.0;
        let temp = splitter * a;
        let hi = temp - (temp - a);
        let lo = a - hi;
        (hi, lo)
    }

    /// Error-free product of two f64s (Dekker's two-product).
    #[inline]
    fn two_prod(a: f64, b: f64) -> (f64, f64) {
        let x = a * b;
        let (a_hi, a_lo) = Self::split(a);
        let (b_hi, b_lo) = Self::split(b);
        let err1 = x - (a_hi * b_hi);
        let err2 = err1 - (a_lo * b_hi);
        let err3 = err2 - (a_hi * b_lo);
        let lo = (a_lo * b_lo) - err3;
        (x, lo)
    }

    /// Renormalize after accumulation to reduce lo component.
    #[inline]
    #[allow(dead_code)]
    fn renormalize(hi: f64, lo: f64) -> Self {
        let (x, e) = Self::two_sum(hi, lo);
        DD { hi: x, lo: e }
    }

    /// DD addition.
    pub fn add(self, other: DD) -> DD {
        let (s_hi, s_lo) = Self::two_sum(self.hi, other.hi);
        let (t_hi, t_lo) = Self::two_sum(self.lo, other.lo);
        let (c1, c2) = Self::two_sum(s_lo, t_lo);
        let (c3, c4) = Self::two_sum(s_hi, c1);
        DD {
            hi: c3,
            lo: c4 + c2 + t_hi,
        }
    }

    /// DD subtraction.
    pub fn sub(self, other: DD) -> DD {
        self.add(DD {
            hi: -other.hi,
            lo: -other.lo,
        })
    }

    /// DD multiplication (uses Dekker's two-product).
    pub fn mul(self, other: DD) -> DD {
        let (p1, p2) = Self::two_prod(self.hi, other.hi);
        let p3 = self.hi * other.lo;
        let p4 = self.lo * other.hi;
        let (x, y) = Self::two_sum(p1, p2 + p3 + p4);
        DD { hi: x, lo: y }
    }

    /// DD × f64 multiplication.
    #[allow(dead_code)]
    pub fn mul_f64(self, other: f64) -> DD {
        let (p1, p2) = Self::two_prod(self.hi, other);
        let p3 = self.lo * other;
        let (x, y) = Self::two_sum(p1, p2 + p3);
        DD { hi: x, lo: y }
    }

    /// DD division using Newton iteration to find f = a/b.
    ///
    /// Method: compute 1/b via Newton, then multiply by a.
    /// Newton iteration for f(x) = b - 1/x: x_{n+1} = x * (2 - b * x).
    pub fn div(self, other: DD) -> DD {
        // Initial approximation of 1/other (using only the high part)
        let inv_approx = 1.0 / other.hi;
        let mut x = DD::from_f64(inv_approx);

        // Two Newton steps for ~106 bits of precision in the reciprocal
        // x = x * (2 - other * x)
        let two = DD::from_f64(2.0);
        x = x.mul(two.sub(other.mul(x)));
        x = x.mul(two.sub(other.mul(x)));

        // self * x = a * (1/b) = a/b
        self.mul(x)
    }

    /// DD cross product: self.x * other.y - self.y * other.x.
    pub fn cross(self_x: DD, self_y: DD, other_x: DD, other_y: DD) -> DD {
        let term1 = self_x.mul(other_y);
        let term2 = self_y.mul(other_x);
        term1.sub(term2)
    }

    /// Dot product: self.x * other.x + self.y * other.y.
    #[allow(dead_code)]
    pub fn dot(self_x: DD, self_y: DD, other_x: DD, other_y: DD) -> DD {
        let term1 = self_x.mul(other_x);
        let term2 = self_y.mul(other_y);
        term1.add(term2)
    }

    /// Squared length (in DD).
    #[allow(dead_code)]
    pub fn sq_len(x: DD, y: DD) -> DD {
        let xx = x.mul(x);
        let yy = y.mul(y);
        xx.add(yy)
    }

    /// Convert to f64 (hi + lo).
    #[inline]
    pub fn to_f64(self) -> f64 {
        self.hi + self.lo
    }

    /// Signum: -1, 0, or 1.
    pub fn signum(self) -> i8 {
        if self.hi > 0.0 {
            1
        } else if self.hi < 0.0 {
            -1
        } else if self.lo > 0.0 {
            1
        } else if self.lo < 0.0 {
            -1
        } else {
            0
        }
    }

    /// Absolute value.
    #[allow(dead_code)]
    pub fn abs(self) -> DD {
        if self.hi < 0.0 || (self.hi == 0.0 && self.lo < 0.0) {
            DD {
                hi: -self.hi,
                lo: -self.lo,
            }
        } else {
            self
        }
    }
}

type NormResult = (
    Coord<f64>,
    Coord<f64>,
    Coord<f64>,
    Coord<f64>,
    f64,
    Coord<f64>,
);

/// Normalize a set of 4 coordinates for numerically stable DD computation.
/// Returns (normalized_coords, scale, origin) where:
/// - Each normalized coord = (coord - origin) / scale
/// - scale is a power of 2 for exact IEEE 754 multiplication/division
pub(crate) fn normalize_four(
    a: Coord<f64>,
    b: Coord<f64>,
    c: Coord<f64>,
    d: Coord<f64>,
) -> NormResult {
    let min_x = a.x.min(b.x).min(c.x).min(d.x);
    let max_x = a.x.max(b.x).max(c.x).max(d.x);
    let min_y = a.y.min(b.y).min(c.y).min(d.y);
    let max_y = a.y.max(b.y).max(c.y).max(d.y);
    let origin = Coord {
        x: (min_x + max_x) / 2.0,
        y: (min_y + max_y) / 2.0,
    };
    let extent = (max_x - min_x).abs().max((max_y - min_y).abs());
    let scale = if extent > 0.0 {
        2.0_f64.powi(extent.log2().ceil() as i32)
    } else {
        1.0
    };
    let norm = |p: Coord<f64>| Coord {
        x: (p.x - origin.x) / scale,
        y: (p.y - origin.y) / scale,
    };
    (norm(a), norm(b), norm(c), norm(d), scale, origin)
}

/// Compute the intersection point of two line segments using double-double
/// arithmetic. Returns `Some((point, t, u))` if the lines intersect at any
/// point (including outside segment bounds), `None` if they are parallel.
///
/// - `t` is the parameter along segment AB (DD precision)
/// - `u` is the parameter along segment CD (DD precision)
/// - `point` is the intersection point (f64)
///
/// Callers should check t/u in the desired range (e.g., `0..1` for interior,
/// `-eps..1+eps` for extended segment intersection).
pub(crate) fn segment_intersection_dd(
    a: Coord<f64>,
    b: Coord<f64>,
    c: Coord<f64>,
    d: Coord<f64>,
) -> Option<(Coord<f64>, DD, DD)> {
    DD_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    // Normalize large coordinates to prevent overflow in DD intermediate products.
    let large_threshold = 1e15;
    let needs_norm = a.x.abs() > large_threshold
        || a.y.abs() > large_threshold
        || b.x.abs() > large_threshold
        || b.y.abs() > large_threshold
        || c.x.abs() > large_threshold
        || c.y.abs() > large_threshold
        || d.x.abs() > large_threshold
        || d.y.abs() > large_threshold;
    let (na, _nb, _nc, _nd, scale, origin) = if needs_norm {
        normalize_four(a, b, c, d)
    } else {
        (a, b, c, d, 1.0, Coord { x: 0.0, y: 0.0 })
    };

    let ax = DD::from_f64(na.x);
    let ay = DD::from_f64(a.y);
    let bx = DD::from_f64(b.x);
    let by = DD::from_f64(b.y);
    let cx = DD::from_f64(c.x);
    let cy = DD::from_f64(c.y);
    let dx = DD::from_f64(d.x);
    let dy = DD::from_f64(d.y);

    // Direction vectors
    let abx = bx.sub(ax);
    let aby = by.sub(ay);
    let cdx = dx.sub(cx);
    let cdy = dy.sub(cy);

    // Denominator = cross(b-a, d-c)
    let denom = DD::cross(abx, aby, cdx, cdy);
    if denom.signum() == 0 {
        return None;
    }

    // t = cross(c-a, d-c) / denom
    let acx = cx.sub(ax);
    let acy = cy.sub(ay);
    let num_t = DD::cross(acx, acy, cdx, cdy);
    let t = num_t.div(denom);

    // u = cross(c-a, b-a) / denom
    let num_u = DD::cross(acx, acy, abx, aby);
    let u = num_u.div(denom);

    // Compute intersection point in DD: p = a + t * (b - a)
    let px = ax.add(abx.mul(t));
    let py = ay.add(aby.mul(t));
    // De-normalize: p = p' * scale + origin
    let result = Coord {
        x: px.to_f64() * scale + origin.x,
        y: py.to_f64() * scale + origin.y,
    };
    Some((result, t, u))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dd_add() {
        let a = DD::from_f64(1.0);
        let b = DD::from_f64(2.0);
        let c = a.add(b);
        assert_eq!(c.to_f64(), 3.0);
        assert_eq!(c.lo, 0.0);
    }

    #[test]
    fn test_dd_sub() {
        let a = DD::from_f64(5.0);
        let b = DD::from_f64(3.0);
        let c = a.sub(b);
        assert_eq!(c.to_f64(), 2.0);
    }

    #[test]
    fn test_dd_mul() {
        let a = DD::from_f64(3.0);
        let b = DD::from_f64(4.0);
        let c = a.mul(b);
        assert_eq!(c.to_f64(), 12.0);
    }

    #[test]
    fn test_dd_div() {
        let a = DD::from_f64(10.0);
        let b = DD::from_f64(3.0);
        let c = a.div(b);
        let expected = 10.0 / 3.0;
        assert!((c.to_f64() - expected).abs() < 1e-15);
    }

    #[test]
    fn test_dd_signum() {
        assert_eq!(DD::from_f64(5.0).signum(), 1);
        assert_eq!(DD::from_f64(-3.0).signum(), -1);
        assert_eq!(DD::from_f64(0.0).signum(), 0);
        let tiny_pos = DD::new(0.0, 1e-200);
        assert_eq!(tiny_pos.signum(), 1);
        let tiny_neg = DD::new(0.0, -1e-200);
        assert_eq!(tiny_neg.signum(), -1);
    }

    #[test]
    fn test_dd_cross() {
        let ax = DD::from_f64(0.0);
        let ay = DD::from_f64(0.0);
        let bx = DD::from_f64(1.0);
        let by = DD::from_f64(1.0);
        let cx = DD::from_f64(0.0);
        let cy = DD::from_f64(0.0);
        let dx = DD::from_f64(1.0);
        let dy = DD::from_f64(0.0);

        let cross = DD::cross(bx.sub(ax), by.sub(ay), dx.sub(cx), dy.sub(cy));
        // cross((1,1), (1,0)) = 1*0 - 1*1 = -1
        assert_eq!(cross.to_f64(), -1.0);
    }

    #[test]
    fn test_segment_intersection_dd_basic() {
        let a = Coord { x: 0.0, y: 0.0 };
        let b = Coord { x: 2.0, y: 2.0 };
        let c = Coord { x: 0.0, y: 2.0 };
        let d = Coord { x: 2.0, y: 0.0 };
        let result = segment_intersection_dd(a, b, c, d);
        assert!(result.is_some());
        let (pt, _, _) = result.unwrap();
        assert!((pt.x - 1.0).abs() < 1e-15);
        assert!((pt.y - 1.0).abs() < 1e-15);
    }

    #[test]
    fn test_segment_intersection_dd_parallel() {
        let a = Coord { x: 0.0, y: 0.0 };
        let b = Coord { x: 1.0, y: 1.0 };
        let c = Coord { x: 2.0, y: 2.0 };
        let d = Coord { x: 3.0, y: 3.0 };
        assert!(segment_intersection_dd(a, b, c, d).is_none());
    }

    #[test]
    fn test_segment_intersection_dd_near_degenerate() {
        // Very near-parallel segments at large coordinates
        let a = Coord { x: 1e10, y: 1e10 };
        let b = Coord {
            x: 1e10 + 1.0,
            y: 1e10 + 1.0000000001,
        };
        let c = Coord {
            x: 1e10,
            y: 1e10 + 1.0,
        };
        let d = Coord {
            x: 1e10 + 1.0,
            y: 1e10 + 0.0,
        };
        let result = segment_intersection_dd(a, b, c, d);
        // These should intersect — cross product is ~1e-10, which f64 can represent
        // but the intersection point should still be computed accurately
        assert!(
            result.is_some(),
            "near-degenerate intersection should be found"
        );
        if let Some((pt, _t, _u)) = result {
            // The intersection point should be near the crossing region
            let eps = 1e-6;
            assert!(
                pt.x >= 1e10 - eps && pt.x <= 1e10 + 2.0 + eps,
                "x out of range: {}",
                pt.x
            );
        }
    }

    #[test]
    fn test_segment_intersection_dd_large_coords() {
        // Large coordinates with precise intersection
        let a = Coord { x: 0.0, y: 0.0 };
        let b = Coord { x: 2e14, y: 2e14 };
        let c = Coord { x: 0.0, y: 2e14 };
        let d = Coord { x: 2e14, y: 0.0 };
        let result = segment_intersection_dd(a, b, c, d);
        assert!(result.is_some());
        let (pt, _, _) = result.unwrap();
        let rel_err_x = (pt.x - 1e14).abs() / 1e14;
        let rel_err_y = (pt.y - 1e14).abs() / 1e14;
        assert!(
            rel_err_x < 1e-10,
            "x relative error too large: {}",
            rel_err_x
        );
        assert!(
            rel_err_y < 1e-10,
            "y relative error too large: {}",
            rel_err_y
        );
    }

    #[test]
    fn test_segment_intersection_dd_very_small_angle() {
        // Two segments that cross at a very small angle (0.001 rad)
        let a = Coord { x: 0.0, y: 0.0 };
        let b = Coord { x: 1000.0, y: 0.0 };
        let small_angle = 0.001f64;
        let c = Coord {
            x: 0.0,
            y: -0.001 * 500.0,
        };
        let d = Coord {
            x: 1000.0,
            y: small_angle * 500.0,
        };
        let result = segment_intersection_dd(a, b, c, d);
        assert!(result.is_some(), "small-angle intersection should be found");
        if let Some((pt, _t, _u)) = result {
            // Should intersect near the middle
            assert!((pt.y).abs() < 1.0, "y should be near 0, got {}", pt.y);
        }
    }

    #[test]
    fn test_dd_sq_len() {
        let x = DD::from_f64(3.0);
        let y = DD::from_f64(4.0);
        let len2 = DD::sq_len(x, y);
        assert_eq!(len2.to_f64(), 25.0);
    }

    #[test]
    fn test_dd_abs() {
        let neg = DD::from_f64(-5.0);
        assert_eq!(neg.abs().to_f64(), 5.0);
        let pos = DD::from_f64(3.0);
        assert_eq!(pos.abs().to_f64(), 3.0);
    }

    #[test]
    fn test_dd_renormalize() {
        let raw = DD::new(1.0, 1e-16);
        let renorm = DD::renormalize(raw.hi, raw.lo);
        assert!((renorm.to_f64() - 1.0000000000000001).abs() < 1e-16);
    }

    #[test]
    fn test_dd_call_counter() {
        // Verify that segment_intersection_dd increments the call counter.
        // Use snapshot deltas to tolerate parallel test interference.
        let before = dd_call_count();
        segment_intersection_dd(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
        );
        let after = dd_call_count();
        assert!(
            after > before,
            "counter should show at least one call (before={before}, after={after})",
        );
        // Reset and verify the counter doesn't show a stale huge number.
        reset_dd_count();
        let reset = dd_call_count();
        assert!(
            reset < 10_000,
            "counter should be near zero after reset, got {reset}",
        );
    }

    #[cfg(feature = "structure")]
    #[test]
    fn test_dd_called_during_polygon_repair() {
        use crate::MakeValid;
        use geo::Polygon;
        // Bowtie polygon — classic self-intersecting case
        let poly = Polygon::new(
            geo::LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        );
        reset_dd_count();
        let _result = poly.make_valid();
        let n = dd_call_count();
        eprintln!("DD calls for bowtie repair: {n}");
        assert!(n > 0, "DD should be called during polygon repair");
    }
}

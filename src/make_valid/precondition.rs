//! Bit-exact preconditioning for uniform input that the snap guard sends to cdt. The
//! guard used max * 1e8 > 2^53. Measured 2026-09-18: large coord 1e12 readme rows ran
//! at 0.08 to 0.12x of geos.

use crate::core::{MakeValidConfig, SNAP_SCALE};
use alloc::vec::Vec;
use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon};

/// Exact power of two `2^exp`, or `None` when out of the normal f64 range.
fn pow2(exp: i32) -> Option<f64> {
    if !(-1022..=1023).contains(&exp) {
        return None;
    }
    Some(f64::from_bits(((exp + 1023) as u64) << 52))
}

fn exp2_floor(x: f64) -> i32 {
    let e = ((x.to_bits() >> 52) & 0x7ff) as i32;
    if e == 0 { -1023 } else { e - 1023 }
}

/// The transform: `t = (c - anchor) * scale`, restored by `c = t * unscale + anchor`.
pub(crate) struct Precondition {
    ax: f64,
    ay: f64,
    scale: f64,
    unscale: f64,
}

impl Precondition {
    /// every coordinate, and (c) the transformed polygon satisfies the
    /// guard's two representability inequalities.
    pub(crate) fn try_new(poly: &Polygon<f64>) -> Option<(Self, Polygon<f64>)> {
        if !crate::make_valid::snap_cannot_represent(poly) {
            return None;
        }
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        let mut any = false;
        for c in poly
            .exterior()
            .0
            .iter()
            .chain(poly.interiors().iter().flat_map(|h| h.0.iter()))
        {
            if !c.x.is_finite() || !c.y.is_finite() {
                return None;
            }
            any = true;
            min_x = min_x.min(c.x);
            min_y = min_y.min(c.y);
            max_x = max_x.max(c.x);
            max_y = max_y.max(c.y);
        }
        if !any {
            return None;
        }
        let span = (max_x - min_x).max(max_y - min_y);
        if !span.is_finite() || span <= 0.0 {
            return None;
        }
        let t = span * SNAP_SCALE;
        if !t.is_finite() {
            return None;
        }
        // scale = 2^-k with two bits of headroom below the guard's 2^53 ceiling.
        let k = exp2_floor(t) - 50;
        let scale = pow2(-k)?;
        let unscale = pow2(k)?;
        let pre = Self {
            ax: min_x,
            ay: min_y,
            scale,
            unscale,
        };
        let mut min_abs = f64::MAX;
        let mut max_abs = 0.0f64;
        let mut map_ring = |ring: &LineString<f64>| -> Option<LineString<f64>> {
            let mut out = Vec::with_capacity(ring.0.len());
            for c in &ring.0 {
                let dx = c.x - pre.ax;
                let dy = c.y - pre.ay;
                if dx + pre.ax != c.x || dy + pre.ay != c.y {
                    return None;
                }
                let tx = dx * pre.scale;
                let ty = dy * pre.scale;
                if !tx.is_finite() || !ty.is_finite() {
                    return None;
                }
                if (dx != 0.0 && tx.abs() < f64::MIN_POSITIVE)
                    || (dy != 0.0 && ty.abs() < f64::MIN_POSITIVE)
                {
                    return None;
                }
                let a = tx.abs().max(ty.abs());
                if a > 0.0 {
                    min_abs = min_abs.min(a);
                }
                max_abs = max_abs.max(a);
                out.push(Coord { x: tx, y: ty });
            }
            Some(LineString::new(out))
        };
        let ext = map_ring(poly.exterior())?;
        let mut holes = Vec::with_capacity(poly.interiors().len());
        for h in poly.interiors() {
            holes.push(map_ring(h)?);
        }
        if max_abs * SNAP_SCALE > (1u64 << 53) as f64 {
            return None;
        }
        if min_abs == f64::MAX || min_abs * SNAP_SCALE < 0.5 {
            return None;
        }
        let tp = Polygon::new(ext, holes);
        debug_assert!(!crate::make_valid::snap_cannot_represent(&tp));
        Some((pre, tp))
    }

    /// Map a repaired geometry back to native magnitude.
    pub(crate) fn inverse_geometry(&self, g: Geometry<f64>) -> Geometry<f64> {
        let map_coord = |c: Coord<f64>| Coord {
            x: c.x * self.unscale + self.ax,
            y: c.y * self.unscale + self.ay,
        };
        let map_ring = |ring: &LineString<f64>| {
            LineString::new(ring.0.iter().map(|c| map_coord(*c)).collect())
        };
        let map_poly = |p: &Polygon<f64>| {
            Polygon::new(
                map_ring(p.exterior()),
                p.interiors().iter().map(map_ring).collect(),
            )
        };
        match g {
            Geometry::Polygon(p) => Geometry::Polygon(map_poly(&p)),
            Geometry::MultiPolygon(mp) => {
                Geometry::MultiPolygon(MultiPolygon::new(mp.0.iter().map(map_poly).collect()))
            }
            Geometry::LineString(ls) => Geometry::LineString(map_ring(&ls)),
            Geometry::MultiLineString(mls) => Geometry::MultiLineString(geo::MultiLineString::new(
                mls.0.iter().map(map_ring).collect(),
            )),
            Geometry::Point(pt) => Geometry::Point(geo::Point(map_coord(pt.0))),
            Geometry::MultiPoint(mpts) => Geometry::MultiPoint(geo::MultiPoint::new(
                mpts.0.iter().map(|p| geo::Point(map_coord(p.0))).collect(),
            )),
            Geometry::GeometryCollection(gc) => {
                Geometry::GeometryCollection(geo::GeometryCollection::new_from(
                    gc.0.into_iter().map(|x| self.inverse_geometry(x)).collect(),
                ))
            }
            other => other,
        }
    }
}

/// Bit-exact preconditioning entry.
#[cfg(feature = "structure")]
pub(crate) fn precondition_fix(
    poly: &Polygon<f64>,
    config: &MakeValidConfig,
) -> Option<crate::structure::FixOutcome> {
    use crate::structure::FixOutcome;
    let (pre, tp) = Precondition::try_new(poly)?;
    match crate::structure::fix_polygon_owned(tp, config, None) {
        FixOutcome::Fast(g, _, _, _) | FixOutcome::Repaired(g) => {
            Some(FixOutcome::Repaired(pre.inverse_geometry(g)))
        }
        FixOutcome::Unconsumed(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(n: usize, r: f64, scale: f64, shift: f64) -> Polygon<f64> {
        let mut coords = Vec::with_capacity(n);
        for i in 0..n - 1 {
            let a = 2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64;
            coords.push(Coord {
                x: (500.0 + r * a.cos()) * scale + shift,
                y: (500.0 + r * a.sin()) * scale + shift,
            });
        }
        coords.push(coords[0]);
        Polygon::new(LineString::new(coords), Vec::new())
    }

    #[test]
    fn accepts_uniform_large_magnitude() {
        let p = circle(64, 100.0, 1e10, 1e12);
        let (_pre, tp) = Precondition::try_new(&p).expect("uniform 1e12 must precondition");
        assert!(!crate::make_valid::snap_cannot_represent(&tp));
    }

    #[test]
    fn accepts_uniform_small_magnitude() {
        let p = circle(64, 100.0, 1e-12, 0.0);
        assert!(
            Precondition::try_new(&p).is_some(),
            "uniform tiny must precondition"
        );
    }

    #[test]
    fn rejects_sub_grid_feature() {
        // A genuine micro feature (1e-12-wide x delta) inside a 1e7 span
        let p = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1e-12, y: 1e-12 },
                Coord { x: 1e7, y: 0.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(Precondition::try_new(&p).is_none());
    }

    #[test]
    fn accepts_small_coords_at_anchor() {
        // 1e-9.1e7 absolute coordinates whose features are all large
        let p = Polygon::new(
            LineString::new(vec![
                Coord { x: 1e7, y: 1e7 },
                Coord { x: 1e-9, y: 1e-9 },
                Coord { x: 1e7, y: 1e-9 },
                Coord { x: 1e-9, y: 1e7 },
                Coord { x: 1e7, y: 1e7 },
            ]),
            Vec::new(),
        );
        assert!(Precondition::try_new(&p).is_some());
    }

    #[test]
    fn rejects_nan_and_empty() {
        let p = Polygon::new(
            LineString::new(vec![
                Coord {
                    x: f64::NAN,
                    y: 0.0,
                },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord {
                    x: f64::NAN,
                    y: 0.0,
                },
            ]),
            Vec::new(),
        );
        assert!(Precondition::try_new(&p).is_none());
        let empty = Polygon::new(LineString::new(Vec::new()), Vec::new());
        assert!(Precondition::try_new(&empty).is_none());
    }

    #[test]
    fn rejects_zero_span() {
        let p = Polygon::new(
            LineString::new(vec![
                Coord { x: 7.0, y: 7.0 },
                Coord { x: 7.0, y: 7.0 },
                Coord { x: 7.0, y: 7.0 },
                Coord { x: 7.0, y: 7.0 },
            ]),
            Vec::new(),
        );
        assert!(Precondition::try_new(&p).is_none());
    }

    #[test]
    fn roundtrip_is_bit_exact() {
        for (scale, shift) in [(1e10, 1e12), (1e-12, 0.0), (1.0, 1e15)] {
            let p = circle(33, 100.0, scale, shift);
            let (pre, tp) = Precondition::try_new(&p).expect("uniform input");
            let back = pre.inverse_geometry(Geometry::Polygon(tp));
            let Geometry::Polygon(bp) = back else {
                panic!("polygon in, polygon out");
            };
            for (a, b) in p.exterior().0.iter().zip(bp.exterior().0.iter()) {
                assert_eq!(
                    (a.x.to_bits(), a.y.to_bits()),
                    (b.x.to_bits(), b.y.to_bits()),
                    "roundtrip must be bit-exact"
                );
            }
        }
    }
}

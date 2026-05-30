use geo::{
    Coord, GeoFloat, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};
use thiserror::Error;

#[derive(Error, Clone, Debug, PartialEq)]
pub enum GeometryValidationError {
    #[error("Coordinate is NaN")]
    CoordinateNaN,

    #[error("Coordinate is infinite")]
    CoordinateInfinite,

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

    #[error("Hole touches shell boundary")]
    HoleTouchesShell,

    #[error("Interior ring is disconnected from shell")]
    DisconnectedInteriorRing,

    #[error("Wrong ring orientation: exterior should be CCW, interior CW")]
    WrongOrientation,

    #[error("Collinear ring: all points lie on a line")]
    CollinearRing,

    #[error("Line has zero length (start == end at {0:?})")]
    ZeroLengthLine(Coord<f64>),

    #[error("Polygon exterior ring is degenerate (collapsed)")]
    DegenerateExterior,
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
    if (max_x - min_x).abs() < f64::EPSILON * scale || (max_y - min_y).abs() < f64::EPSILON * scale
    {
        if is_exterior {
            errors.push(GeometryValidationError::DegenerateExterior);
        } else {
            errors.push(GeometryValidationError::CollinearRing);
        }
        return errors;
    }

    let mut seen = rustc_hash::FxHashSet::with_capacity_and_hasher(n, Default::default());
    for c in &ring[..n] {
        if !seen.insert((c.x.to_bits(), c.y.to_bits())) {
            errors.push(GeometryValidationError::PinchPoint);
            break;
        }
    }

    let eps = 1e-12 * scale;
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

    errors
}

fn check_edge_pair_intersection(coords: &[Coord<f64>], i: usize, j: usize, eps: f64) -> bool {
    let a1 = coords[i];
    let a2 = coords[(i + 1) % (coords.len() - 1)];
    let b1 = coords[j];
    let b2 = coords[(j + 1) % (coords.len() - 1)];

    let o1 = (a2.x - a1.x) * (b1.y - a1.y) - (a2.y - a1.y) * (b1.x - a1.x);
    let o2 = (a2.x - a1.x) * (b2.y - a1.y) - (a2.y - a1.y) * (b2.x - a1.x);
    let o3 = (b2.x - b1.x) * (a1.y - b1.y) - (b2.y - b1.y) * (a1.x - b1.x);
    let o4 = (b2.x - b1.x) * (a2.y - b1.y) - (b2.y - b1.y) * (a2.x - b1.x);

    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }

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

fn check_holes_valid(
    shell: &[Coord<f64>],
    interiors: &[LineString<f64>],
) -> Vec<GeometryValidationError> {
    let mut errors = Vec::new();
    for hole in interiors {
        if let Some(pt) = hole.0.first().copied() {
            if !point_in_ring_exclusive(pt, shell) {
                errors.push(GeometryValidationError::HoleOutsideShell);
            }
        }
    }
    let holes: Vec<&[Coord<f64>]> = interiors.iter().map(|h| &h.0[..]).collect();
    for (i, h1) in holes.iter().enumerate() {
        for h2 in holes.iter().skip(i + 1) {
            if let Some(pt) = h2.first().copied() {
                if point_in_ring_exclusive(pt, h1) {
                    errors.push(GeometryValidationError::NestedHoles);
                    return errors;
                }
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
        ValidationResult::valid()
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
        let area = (coords[1].x - coords[0].x) * (coords[2].y - coords[0].y)
            - (coords[1].y - coords[0].y) * (coords[2].x - coords[0].x);
        if area == 0.0 {
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
        for (i, s1) in shells.iter().enumerate() {
            for s2 in shells.iter().skip(i + 1) {
                if let Some(pt) = s2.first().copied() {
                    if point_in_ring_exclusive(pt, s1) {
                        errors.push(GeometryValidationError::NestedHoles);
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
        let mut errors = Vec::new();
        for g in &self.0 {
            let r = g.validate();
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
}

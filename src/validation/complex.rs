#[cfg_attr(not(test), allow(unused_imports))]
use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};

use crate::validation::core::*;

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
        // Use fingerprint grouping to avoid O(h²) pairwise checks.
        if interiors.len() > 1 {
            let mut groups: rustc_hash::FxHashMap<(usize, u64), Vec<usize>> =
                rustc_hash::FxHashMap::with_capacity_and_hasher(
                    interiors.len(),
                    Default::default(),
                );
            for (i, h) in interiors.iter().enumerate() {
                groups.entry(ring_dup_fingerprint(h)).or_default().push(i);
            }
            for (_, indices) in groups {
                for (ii, &a) in indices.iter().enumerate() {
                    for &b in indices.iter().skip(ii + 1) {
                        if is_rotated_duplicate(interiors[a], interiors[b]) {
                            errors.push(GeometryValidationError::DuplicatedRings);
                            return ValidationResult::invalid(errors);
                        }
                    }
                }
            }
        }
        // Also check each hole against the shell
        for h in &interiors {
            if ring_dup_fingerprint(h) == ring_dup_fingerprint(&self.exterior().0)
                && is_rotated_duplicate(h, &self.exterior().0)
            {
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
        if shells.len() > 1 {
            let mut groups: rustc_hash::FxHashMap<(usize, u64), Vec<usize>> =
                rustc_hash::FxHashMap::with_capacity_and_hasher(shells.len(), Default::default());
            for (i, s) in shells.iter().enumerate() {
                groups.entry(ring_dup_fingerprint(s)).or_default().push(i);
            }
            for (_, indices) in groups {
                for (ii, &a) in indices.iter().enumerate() {
                    for &b in indices.iter().skip(ii + 1) {
                        if is_rotated_duplicate(shells[a], shells[b]) {
                            errors.push(GeometryValidationError::DuplicatedRings);
                            return ValidationResult::invalid(errors);
                        }
                    }
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
            #[cfg(feature = "rstar")]
            {
                struct ShellEnv {
                    idx: usize,
                    env: rstar::AABB<[f64; 2]>,
                }
                impl rstar::RTreeObject for ShellEnv {
                    type Envelope = rstar::AABB<[f64; 2]>;
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
                        env: rstar::AABB::from_corners([min_x, min_y], [max_x, max_y]),
                    });
                }
                let tree = rstar::RTree::bulk_load(envs);
                for (i, s2) in shells.iter().enumerate() {
                    let Some(pt) = s2.first().copied() else {
                        continue;
                    };
                    let query = rstar::AABB::from_corners([pt.x, pt.y], [pt.x, pt.y]);
                    let mut overlaps = false;
                    let _ = tree.locate_in_envelope_intersecting_int(query, |c| {
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
            #[cfg(not(feature = "rstar"))]
            {
                for i in 0..shells.len() {
                    for j in 0..shells.len() {
                        if i == j {
                            continue;
                        }
                        if let Some(pt) = shells[j].first().copied()
                            && point_in_ring_exclusive(pt, shells[i]) {
                            errors.push(GeometryValidationError::NestedHoles);
                            return ValidationResult::invalid(errors);
                        }
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
                // OGC: GeometryCollection doesn't enforce simplicity.
                // Filter out NotSimple from sub-geometry validation.
                for e in r.errors {
                    if !matches!(e, GeometryValidationError::NotSimple) {
                        errors.push(e);
                    }
                }
            }
        }
        for i in 0..self.0.len() {
            for j in (i + 1)..self.0.len() {
                if self.0[i] == self.0[j] {
                    errors.push(GeometryValidationError::DuplicatedRings);
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
        // Point(NaN, NaN) is the geo representation of POINT EMPTY — valid OGC
        assert!(Point::new(f64::NAN, 2.0).is_valid());
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
        assert!(g2.is_valid());
    }

    #[test]
    fn test_geometry_collection() {
        let gc = GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::Point(Point::new(f64::NAN, 2.0)),
        ]);
        assert!(gc.is_valid());
        assert_eq!(gc.validate().errors.len(), 0);
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

    #[test]
    fn test_pinch_point() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        let result = poly.validate();
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::PinchPoint)));
    }

    #[test]
    fn test_nested_holes() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 50.0, y: 0.0 },
                Coord { x: 50.0, y: 50.0 },
                Coord { x: 0.0, y: 50.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![
                LineString::new(vec![
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 45.0, y: 5.0 },
                    Coord { x: 45.0, y: 45.0 },
                    Coord { x: 5.0, y: 45.0 },
                    Coord { x: 5.0, y: 5.0 },
                ]),
                LineString::new(vec![
                    Coord { x: 10.0, y: 10.0 },
                    Coord { x: 20.0, y: 10.0 },
                    Coord { x: 20.0, y: 20.0 },
                    Coord { x: 10.0, y: 20.0 },
                    Coord { x: 10.0, y: 10.0 },
                ]),
            ],
        );
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::NestedHoles)));
    }

    #[test]
    fn test_disconnected_interior_ring() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 50.0, y: 0.0 },
                Coord { x: 50.0, y: 50.0 },
                Coord { x: 0.0, y: 50.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![
                LineString::new(vec![
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 45.0, y: 5.0 },
                    Coord { x: 45.0, y: 45.0 },
                    Coord { x: 5.0, y: 45.0 },
                    Coord { x: 5.0, y: 5.0 },
                ]),
                LineString::new(vec![
                    Coord { x: 8.0, y: 8.0 },
                    Coord { x: 12.0, y: 8.0 },
                    Coord { x: 12.0, y: 12.0 },
                    Coord { x: 8.0, y: 12.0 },
                    Coord { x: 8.0, y: 8.0 },
                ]),
            ],
        );
        let errors = &poly.validate().errors;
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, GeometryValidationError::NestedHoles))
                || errors
                    .iter()
                    .any(|e| matches!(e, GeometryValidationError::DisconnectedInteriorRing)),
            "expected NestedHoles or DisconnectedInteriorRing, got: {errors:?}",
        );
    }

    #[test]
    fn test_wrong_orientation_shell_cw() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::WrongOrientation)));
    }

    #[test]
    fn test_repeated_point_in_ring() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::RepeatedPoint)));
    }

    #[test]
    fn test_duplicated_rings() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 20.0, y: 0.0 },
                Coord { x: 20.0, y: 20.0 },
                Coord { x: 0.0, y: 20.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![
                LineString::new(vec![
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 5.0, y: 15.0 },
                    Coord { x: 15.0, y: 15.0 },
                    Coord { x: 15.0, y: 5.0 },
                    Coord { x: 5.0, y: 5.0 },
                ]),
                LineString::new(vec![
                    Coord { x: 5.0, y: 5.0 },
                    Coord { x: 5.0, y: 15.0 },
                    Coord { x: 15.0, y: 15.0 },
                    Coord { x: 15.0, y: 5.0 },
                    Coord { x: 5.0, y: 5.0 },
                ]),
            ],
        );
        assert!(!poly.is_valid());
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::DuplicatedRings)));
    }

    #[test]
    fn test_multipoint_duplicate_points() {
        let mp = MultiPoint::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
            Point::new(1.0, 2.0),
        ]);
        assert!(!mp.is_valid());
        assert!(mp
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::MultiPointDuplicatePoints)));
    }

    #[test]
    fn test_multilinestring_duplicate_lines() {
        let mls = MultiLineString::new(vec![
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 }]),
            LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 }]),
        ]);
        assert!(!mls.is_valid());
        assert!(mls
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::MultiLineStringDuplicateLines)));
    }

    #[test]
    fn test_degenerate_exterior_collinear_x() {
        let poly = Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 20.0, y: 0.0 },
                Coord { x: 30.0, y: 0.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        );
        assert!(!poly.is_valid());
        assert!(poly
            .validate()
            .errors
            .iter()
            .any(|e| matches!(e, GeometryValidationError::DegenerateExterior)));
    }
}

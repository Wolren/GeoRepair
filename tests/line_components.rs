//! MultiLineString repair keeps every component: point contacts are noded, duplicates are
//! deduplicated, and no part is dropped for touching another.
use geo::{Coord, Geometry, LineString, MultiLineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig};

fn ls(points: &[(f64, f64)]) -> LineString<f64> {
    LineString::new(points.iter().map(|&(x, y)| Coord { x, y }).collect())
}

fn mls(parts: &[&[(f64, f64)]]) -> MultiLineString<f64> {
    MultiLineString::new(parts.iter().map(|p| ls(p)).collect())
}

fn length(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::LineString(l) => l
            .0
            .windows(2)
            .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
            .sum(),
        Geometry::MultiLineString(m) => m.0.iter().map(|l| length(&Geometry::LineString(l.clone()))).sum(),
        Geometry::GeometryCollection(gc) => gc.0.iter().map(length).sum(),
        _ => 0.0,
    }
}

fn components(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::LineString(_) => 1,
        Geometry::MultiLineString(m) => m.0.len(),
        Geometry::GeometryCollection(gc) => gc.0.len(),
        _ => 1,
    }
}

fn repair(g: &Geometry<f64>) -> Geometry<f64> {
    g.make_valid_with_config(&MakeValidConfig::default())
}

#[test]
fn crossing_parts_are_noded_not_dropped() {
    // Two parts cross at (50, 0): each is split there and all four pieces stay.
    let g = Geometry::MultiLineString(mls(&[&[(0.0, 0.0), (100.0, 0.0)], &[(50.0, -50.0), (50.0, 50.0)]]));
    let out = repair(&g);
    assert!(out.validate().valid, "noded output must be valid: {out:?}");
    assert_eq!(components(&out), 4, "both parts split at the crossing: {out:?}");
    assert!((length(&out) - 200.0).abs() < 1e-9, "length must survive: {out:?}");
}

#[test]
fn endpoint_on_part_is_noded() {
    // Part 2 ends in the middle of part 1: part 1 is split there and the contact becomes
    // an endpoint of every piece.
    let g = Geometry::MultiLineString(mls(&[&[(0.0, 0.0), (100.0, 0.0)], &[(50.0, 50.0), (50.0, 0.0)]]));
    let want = length(&g);
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_eq!(components(&out), 3, "part 1 split at (50, 0): {out:?}");
    assert!((length(&out) - want).abs() < 1e-9, "length must survive: {out:?}");
}

#[test]
fn identical_parts_are_deduplicated() {
    let g = Geometry::MultiLineString(mls(&[&[(0.0, 0.0), (100.0, 0.0)], &[(0.0, 0.0), (100.0, 0.0)]]));
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_eq!(components(&out), 1, "the duplicate is dropped, not the geometry: {out:?}");
    assert!((length(&out) - 100.0).abs() < 1e-9, "length: {out:?}");
}

#[test]
fn partial_collinear_overlap_keeps_the_union_length() {
    // 0..100 and 50..200 share 50..100 once.
    let g = Geometry::MultiLineString(mls(&[&[(0.0, 0.0), (100.0, 0.0)], &[(50.0, 0.0), (200.0, 0.0)]]));
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert!((length(&out) - 200.0).abs() < 1e-9, "union length 200: {out:?}");
}

#[test]
fn parts_meeting_at_endpoints_are_unchanged() {
    let g = Geometry::MultiLineString(mls(&[&[(0.0, 0.0), (100.0, 0.0)], &[(0.0, 0.0), (50.0, 50.0)]]));
    assert!(g.validate().valid, "endpoint contact is valid");
    let out = repair(&g);
    assert_eq!(components(&out), 2, "nothing to do: {out:?}");
    assert!((length(&out) - (100.0 + 50.0f64.hypot(50.0))).abs() < 1e-9);
}

#[test]
fn closed_part_touching_an_open_part_keeps_coverage() {
    // A closed component has no boundary, so it cannot touch anything; it is cut into two
    // open paths at the contact point, and no segment is lost.
    let g = Geometry::MultiLineString(mls(&[
        &[(10.0, 10.0), (20.0, 20.0)],
        &[(20.0, 20.0), (20.0, 30.0), (30.0, 30.0), (30.0, 20.0), (20.0, 20.0)],
    ]));
    let want = length(&g);
    let out = repair(&g);
    assert!(out.validate().valid, "output must be valid: {out:?}");
    assert_eq!(components(&out), 3, "the closed square becomes two open paths: {out:?}");
    assert!((length(&out) - want).abs() < 1e-9, "length must survive: {out:?}");
}

#[test]
fn retrace_ring_is_invalid_and_repaired() {
    // A ring that runs back along part of its own previous edge: invalid, and the repair
    // must drop the retrace, not keep it.
    let ring = ls(&[
        (0.0, 5.0),
        (999901360038144.0, 1.5),
        (0.0, 589338232487936.0),
        (0.0, -5.500000000232603),
        (0.0, 5.0),
    ]);
    let g = Geometry::Polygon(Polygon::new(ring, Vec::new()));
    assert!(!g.validate().valid, "the retrace must be rejected");
    let out = repair(&g);
    assert!(out.validate().valid, "repair must produce a valid ring: {out:?}");
    assert!(length_polygon_ring(&out) > 0.0, "the thin sliver survives: {out:?}");
}

fn length_polygon_ring(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::Polygon(p) => p.exterior().0.len() as f64,
        Geometry::MultiPolygon(m) => m.0.iter().map(|p| p.exterior().0.len() as f64).sum(),
        _ => 0.0,
    }
}

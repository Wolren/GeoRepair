//! Contract tests for `docs/REPAIR-CORE-SPEC.md`.
//!
//! These pin the even-odd area contract and the MakeValid loop independently of
//! the implementation that currently provides them.

use geo::{Area, Coord, Line, LineString, MultiPolygon, Polygon};
use geo_repair::structure::build_area::build_area;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod, is_valid};

fn ring(coords: &[(f64, f64)]) -> Vec<Line<f64>> {
    coords
        .windows(2)
        .map(|w| {
            Line::new(
                Coord {
                    x: w[0].0,
                    y: w[0].1,
                },
                Coord {
                    x: w[1].0,
                    y: w[1].1,
                },
            )
        })
        .collect()
}

fn square(x: f64, y: f64, w: f64) -> Vec<Line<f64>> {
    ring(&[(x, y), (x + w, y), (x + w, y + w), (x, y + w), (x, y)])
}

fn area(lines: &[Line<f64>]) -> f64 {
    build_area(lines)
        .expect("build_area returns None only on internal failure")
        .unsigned_area()
}

#[test]
fn square_keeps_its_area() {
    assert!((area(&square(0.0, 0.0, 10.0)) - 100.0).abs() < 1e-9);
}

#[test]
fn hole_is_subtracted_and_island_is_kept() {
    let mut lines = square(0.0, 0.0, 10.0);
    lines.extend(square(3.0, 3.0, 4.0));
    assert!((area(&lines) - 84.0).abs() < 1e-9);

    lines.extend(square(4.0, 4.0, 2.0));
    assert!((area(&lines) - 88.0).abs() < 1e-9);
}

fn make_valid(poly: &Polygon<f64>, method: PolyMethod) -> geo::Geometry<f64> {
    let config = MakeValidConfig {
        poly_method: method,
        ..Default::default()
    };
    poly.make_valid_with_config(&config)
}

#[test]
fn doubled_ring_has_no_interior() {
    // The ring walks the square twice: every crossing count is even, so the
    // even-odd interior is empty.
    let coords = vec![
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ];
    let poly = Polygon::new(
        LineString::new(
            coords
                .iter()
                .map(|&(x, y)| Coord { x, y })
                .collect::<Vec<_>>(),
        ),
        Vec::new(),
    );
    for method in [PolyMethod::Auto, PolyMethod::Arrange] {
        let out = make_valid(&poly, method);
        assert!(
            out.unsigned_area() < 1e-9,
            "{method:?}: area {}",
            out.unsigned_area()
        );
    }
}

#[test]
fn pentagram_centre_is_empty() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 2.0, y: 3.0 },
            Coord { x: 4.0, y: 0.0 },
            Coord { x: 0.0, y: 2.0 },
            Coord { x: 4.0, y: 2.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    for method in [PolyMethod::Structure, PolyMethod::Arrange] {
        let out = make_valid(&poly, method);
        assert!(
            (out.unsigned_area() - 3.333_333_333).abs() < 1e-6,
            "{method:?}: area {}",
            out.unsigned_area()
        );
    }
}

#[test]
fn dangling_edge_is_ignored() {
    let mut lines = square(0.0, 0.0, 10.0);
    lines.push(Line::new(
        Coord { x: 10.0, y: 10.0 },
        Coord { x: 20.0, y: 20.0 },
    ));
    assert!((area(&lines) - 100.0).abs() < 1e-9);
}

#[test]
fn empty_input_is_empty() {
    let out = build_area(&[]).expect("empty input is a valid call");
    assert!(out.0.is_empty());
}

#[test]
fn output_is_deterministic() {
    let lines = ring(&[
        (0.0, 0.0),
        (4.0, 4.0),
        (4.0, 0.0),
        (0.0, 4.0),
        (0.0, 0.0),
        (2.0, 0.0),
        (2.0, 4.0),
        (0.0, 4.0),
        (0.0, 0.0),
    ]);
    let a = build_area(&lines).unwrap();
    let b = build_area(&lines).unwrap();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

#[test]
fn make_valid_is_idempotent_on_a_bowtie() {
    let bowtie = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let config = MakeValidConfig {
        poly_method: PolyMethod::Auto,
        ..Default::default()
    };
    let first = bowtie.make_valid_with_config(&config);
    assert!(is_valid(&first));
    let second = match &first {
        geo::Geometry::Polygon(p) => p.make_valid_with_config(&config),
        geo::Geometry::MultiPolygon(mp) => mp.make_valid_with_config(&config),
        other => panic!("unexpected repair output: {other:?}"),
    };
    assert!((first.unsigned_area() - second.unsigned_area()).abs() < 1e-9);
    assert!(is_valid(&second));
}

/// The even-odd area is a property of the boundary, not of the part order.
#[test]
fn overlapping_parts_cancel_in_the_even_odd_area() {
    let a = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 4.0, y: 0.0 },
            Coord { x: 4.0, y: 4.0 },
            Coord { x: 0.0, y: 4.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let b = Polygon::new(
        LineString::new(vec![
            Coord { x: 2.0, y: 2.0 },
            Coord { x: 6.0, y: 2.0 },
            Coord { x: 6.0, y: 6.0 },
            Coord { x: 2.0, y: 6.0 },
            Coord { x: 2.0, y: 2.0 },
        ]),
        Vec::new(),
    );
    let mp = MultiPolygon::new(vec![a, b]);
    let out = mp.make_valid_with_config(&MakeValidConfig {
        poly_method: PolyMethod::Auto,
        ..Default::default()
    });
    assert!(
        (out.unsigned_area() - 24.0).abs() < 1e-9,
        "even-odd area {}",
        out.unsigned_area()
    );
    assert!(is_valid(&out));
}

//! Twin agreement: the borrowed `MakeValid::make_valid_with_config` entry
//! and the owned `make_valid_owned` twin must return bit-identical results
//! on every input class. Both bodies delegate to the same shared helpers
//! (dedup 2026-09-04); this test pins that contract so the twins can never
//! silently drift apart again.

use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::make_valid::make_valid_owned;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fn cfg_with(method: PolyMethod, keep_collapsed: bool) -> MakeValidConfig {
    MakeValidConfig {
        poly_method: method,
        keep_collapsed,
        ..Default::default()
    }
}

fn coord_eq(a: &Coord<f64>, b: &Coord<f64>) -> bool {
    (a.x.to_bits() == b.x.to_bits() || (a.x.is_nan() && b.x.is_nan()))
        && (a.y.to_bits() == b.y.to_bits() || (a.y.is_nan() && b.y.is_nan()))
}

fn ls_eq(a: &LineString<f64>, b: &LineString<f64>) -> bool {
    a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| coord_eq(x, y))
}

fn poly_eq(a: &Polygon<f64>, b: &Polygon<f64>) -> bool {
    ls_eq(a.exterior(), b.exterior())
        && a.interiors().len() == b.interiors().len()
        && a.interiors()
            .iter()
            .zip(b.interiors())
            .all(|(x, y)| ls_eq(x, y))
}

fn geom_eq(a: &Geometry<f64>, b: &Geometry<f64>) -> bool {
    match (a, b) {
        (Geometry::Point(p), Geometry::Point(q)) => coord_eq(&p.0, &q.0),
        (Geometry::Line(l), Geometry::Line(m)) => {
            coord_eq(&l.start, &m.start) && coord_eq(&l.end, &m.end)
        }
        (Geometry::LineString(l), Geometry::LineString(m)) => ls_eq(l, m),
        (Geometry::Polygon(p), Geometry::Polygon(q)) => poly_eq(p, q),
        (Geometry::MultiPoint(a), Geometry::MultiPoint(b)) => {
            a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| coord_eq(&x.0, &y.0))
        }
        (Geometry::MultiLineString(a), Geometry::MultiLineString(b)) => {
            a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| ls_eq(x, y))
        }
        (Geometry::MultiPolygon(a), Geometry::MultiPolygon(b)) => {
            a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| poly_eq(x, y))
        }
        (Geometry::GeometryCollection(a), Geometry::GeometryCollection(b)) => {
            a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(x, y)| geom_eq(x, y))
        }
        (Geometry::Rect(a), Geometry::Rect(b)) => {
            coord_eq(&a.min(), &b.min()) && coord_eq(&a.max(), &b.max())
        }
        (Geometry::Triangle(a), Geometry::Triangle(b)) => {
            coord_eq(&a.v1(), &b.v1()) && coord_eq(&a.v2(), &b.v2()) && coord_eq(&a.v3(), &b.v3())
        }
        _ => false,
    }
}

fn ring_of(pts: &[(f64, f64)]) -> LineString<f64> {
    LineString::new(pts.iter().map(|(x, y)| Coord { x: *x, y: *y }).collect())
}

/// Every input class the entry pre-scan branches on: valid CCW/CW, holes,
/// self-touch, bowtie, NaN shell/hole, short/collinear/repeated rings,
/// scaled magnitudes, and a 500-vertex valid ring.
fn corpus() -> Vec<Polygon<f64>> {
    let sq = |cw: bool| {
        let mut pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        if cw {
            pts.reverse();
        }
        pts.push(pts[0]);
        Polygon::new(ring_of(&pts), vec![])
    };
    let mut v = vec![
        sq(false),
        sq(true),
        Polygon::new(
            ring_of(&[
                (0.0, 0.0),
                (20.0, 0.0),
                (20.0, 20.0),
                (0.0, 20.0),
                (0.0, 0.0),
            ]),
            vec![ring_of(&[
                (5.0, 5.0),
                (5.0, 15.0),
                (15.0, 15.0),
                (15.0, 5.0),
                (5.0, 5.0),
            ])],
        ),
        // Bowtie (proper crossing).
        Polygon::new(
            ring_of(&[
                (0.0, 0.0),
                (10.0, 10.0),
                (10.0, 0.0),
                (0.0, 10.0),
                (0.0, 0.0),
            ]),
            vec![],
        ),
        // NaN in the shell.
        Polygon::new(
            ring_of(&[
                (0.0, 0.0),
                (f64::NAN, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (0.0, 0.0),
            ]),
            vec![],
        ),
        // NaN confined to a hole.
        Polygon::new(
            ring_of(&[
                (0.0, 0.0),
                (20.0, 0.0),
                (20.0, 20.0),
                (0.0, 20.0),
                (0.0, 0.0),
            ]),
            vec![ring_of(&[
                (5.0, 5.0),
                (f64::NAN, 15.0),
                (15.0, 15.0),
                (15.0, 5.0),
                (5.0, 5.0),
            ])],
        ),
        // Short ring (2 distinct verts + closure).
        Polygon::new(ring_of(&[(1.0, 2.0), (3.0, 4.0), (1.0, 2.0)]), vec![]),
        // Fully collinear ring (collapses).
        Polygon::new(
            ring_of(&[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (0.0, 0.0)]),
            vec![],
        ),
        // Repeated consecutive points.
        Polygon::new(
            ring_of(&[
                (0.0, 0.0),
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (0.0, 0.0),
            ]),
            vec![],
        ),
        // Huge-magnitude valid shell.
        Polygon::new(
            ring_of(&[(100.0, 100.0), (1e15, 110.0), (1e15, 100.0), (100.0, 100.0)]),
            vec![],
        ),
        // Micro-sliver at unit scale.
        Polygon::new(
            ring_of(&[
                (0.0, 0.0),
                (1e-10, 0.0),
                (1e-10, 1e-10),
                (0.0, 1e-10),
                (0.0, 0.0),
            ]),
            vec![],
        ),
    ];
    // 500-vertex valid ring (gate pinch-table path, n > 32).
    let mut big = Vec::with_capacity(501);
    for i in 0..500 {
        let a = 2.0 * std::f64::consts::PI * i as f64 / 500.0;
        big.push((100.0 * a.cos(), 100.0 * a.sin()));
    }
    big.push(big[0]);
    v.push(Polygon::new(ring_of(&big), vec![]));
    v
}

fn configs() -> Vec<(&'static str, MakeValidConfig)> {
    vec![
        ("auto", cfg_with(PolyMethod::Auto, false)),
        ("structure", cfg_with(PolyMethod::Structure, false)),
        ("arrange", cfg_with(PolyMethod::Arrange, false)),
        ("auto-keep", cfg_with(PolyMethod::Auto, true)),
        ("structure-keep", cfg_with(PolyMethod::Structure, true)),
    ]
}

#[test]
fn twin_entries_agree_bit_exact() {
    let ps = corpus();
    for (name, cfg) in configs() {
        for (i, p) in ps.iter().enumerate() {
            let a = p.make_valid_with_config(&cfg);
            let b = make_valid_owned(p.clone(), &cfg);
            assert!(
                geom_eq(&a, &b),
                "twin mismatch: config={name} corpus[{i}] borrowed={a:?} owned={b:?}"
            );
        }
    }
}

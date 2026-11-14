//! Regression: repair must preserve the even-odd interior on self-crossing walks.
//!
//! A valid output with the wrong area is the failure these tests guard. The expected
//! areas were computed outside this crate on 2026-11-14: fine-grid crossing parity and
//! shapely's polygonize agree, and Arrange reproduces both to 1e-9. Structure used to
//! return 193.68 and 212.74 on these rings, losing 81 percent of the second interior.

use geo::{Area, Coord, Geometry, LineString, Polygon};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod, is_valid};

/// Verbatim replication of `benches/bench.rs::make_spaghetti_ring`.
fn spaghetti_ring(n: usize, seed0: u64) -> Polygon<f64> {
    let span = (n as f64).sqrt().ceil() as i64;
    let mut x = 0i64;
    let mut y = 0i64;
    let mut seed = seed0;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as u32
    };
    let mut coords = Vec::with_capacity(n + 1);
    coords.push(Coord {
        x: x as f64,
        y: y as f64,
    });
    for _ in 1..n {
        match next() % 4 {
            0 => x += 1,
            1 => x -= 1,
            2 => y += 1,
            _ => y -= 1,
        }
        x = x.rem_euclid(span);
        y = y.rem_euclid(span);
        coords.push(Coord {
            x: x as f64,
            y: y as f64,
        });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

fn repair(poly: &Polygon<f64>, method: PolyMethod) -> Geometry<f64> {
    let config = MakeValidConfig {
        poly_method: method,
        ..Default::default()
    };
    poly.make_valid_with_config(&config)
}

fn area(g: &Geometry<f64>) -> f64 {
    g.unsigned_area()
}

/// Every method applies the same EvenOdd rule, so every method must return the
/// even-odd area of the input boundary.
#[test]
fn every_method_preserves_the_even_odd_area() {
    for (n, expected) in [(500usize, 193.045455f64), (2000, 1122.269231f64)] {
        let poly = spaghetti_ring(n, 0x9E37_79B9_7F4A_7C15);
        for method in [PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
            let out = repair(&poly, method);
            assert!(is_valid(&out), "n={n} {method:?}: repair must stay valid");
            let got = area(&out);
            assert!(
                (got - expected).abs() <= 1e-6 * expected,
                "n={n} {method:?}: area {got} differs from the even-odd {expected}"
            );
        }
    }
}

/// Random walks with coincident segments are the class where Structure's face
/// assembly used to drop coverage. Both strategies claim EvenOdd, so their areas
/// must agree on every walk.
#[test]
fn strategies_agree_on_random_crossing_walks() {
    for (k, n) in [41usize, 90, 160, 260].into_iter().enumerate() {
        let seed = 0x9E37_79B9_7F4A_7C15u64.wrapping_add((k as u64) * 0x0100_0000_01B3);
        let poly = spaghetti_ring(n, seed);
        let structure = repair(&poly, PolyMethod::Structure);
        let arrange = repair(&poly, PolyMethod::Arrange);
        assert!(is_valid(&structure), "n={n}: Structure output invalid");
        assert!(is_valid(&arrange), "n={n}: Arrange output invalid");
        let (a, b) = (area(&structure), area(&arrange));
        let scale = a.abs().max(b.abs()).max(1e-9);
        assert!(
            (a - b).abs() <= 1e-6 * scale,
            "n={n}: Structure area {a} against Arrange area {b}"
        );
    }
}

//! Regression: repair must preserve the even-odd interior on self-crossing walks.
//!
//! A valid output with the wrong area is the failure these tests guard. The expected
//! areas were computed outside this crate on 2026-11-14: fine-grid crossing parity and
//! shapely's polygonize agree, and Arrange reproduces both to 1e-9. Structure used to
//! return 193.68 and 212.74 on these rings, losing 81 percent of the second interior.

use geo::{Area, Coord, Geometry, LineString, Polygon};
use geo_repair::io::wkt::read_wkt;
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

/// Measured 2026-11-17 against the differential harness: each of these was a valid
/// output with the wrong even-odd interior before the fix that this test now guards.
#[test]
fn measured_valid_but_wrong_cases_reach_the_even_odd_area() {
    let cases: [(&str, &str, f64); 7] = [
        (
            "fuzz/mut13",
            "POLYGON ((0.0 -0.8749895095825195, 6.0 1.5, -6.0 1.5, \
             -3.0 -1.0000038148016301, 9.779748101611e-311 -5.5, 0.0 -3.07568359375, \
             7.7951385e-317 -832.0000000001164, 3.0 1e-323, 1.7800590868057611e-307 1.500000000349246, \
             2.0 -3.000019073486328, 0.0 2.01611328125, 0.0 -0.8749895095825195))",
            1269.282452969912,
        ),
        (
            "geos-590",
            "POLYGON ((-85.07506071796652 113.86542584944674, 64.36729658991037 156.01073329162756, \
             137.30796566724 43.25774389276705, 87.88516621432945 -9.372434717166783, \
             64.10432752471603 12.534535164377898, 57.62905661272339 11.854058704416062, \
             56.45159362282983 23.058548935912473, 66.28625345902752 31.88392771492336, \
             77.29837628511876 29.50481626243784, 75.61280880721095 21.702879922299704, \
             88.61134125246906 -8.599133277701018, 55.132052204961596 -44.25111944215836, \
             38.706619661774155 -28.826682741576086, 20.046266118443334 -41.45598546971478, \
             -85.07506071796652 113.86542584944674), \
             (30.042566164341164 24.175640312260136, 36.69488467846846 54.96703029918906, \
             73.55504106249744 58.84060857425233, 44.592542570140935 103.61135618024262, \
             -12.770587939057421 87.43396969686786, 30.042566164341164 24.175640312260136))",
            21322.016175074536,
        ),
        (
            "buffer-1 part 25",
            "MULTIPOLYGON (((708258.754920656 2402197.91172757, 708257.029447455 2402206.56901508, \
             708652.961095455 2402312.65463437, 708657.068786251 2402304.6356364, \
             708258.754920656 2402197.91172757)), ((708653.498611049 2402311.54647056, \
             708708.895756966 2402203.47250014, 708280.326454234 2402089.6337791, \
             708247.896591321 2402252.48269854, 708367.379593851 2402324.00761653, \
             708248.882609455 2402253.07294874, 708249.523621829 2402244.3124463, \
             708261.854734465 2402182.39086576, 708262.818392579 2402183.35452387, \
             708653.498611049 2402311.54647056)))",
            48364.768163718174,
        ),
        (
            "nested holes",
            "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), \
             (30 70, 70 70, 70 30, 30 30, 30 70), (20 80, 80 80, 80 20, 20 20, 20 80))",
            4400.0,
        ),
        (
            "nested shells",
            "MULTIPOLYGON (((10 10, 20 30, 10 90, 90 90, 80 30, 90 10, 50 20, 10 10)), \
             ((80 30, 20 30, 50 20, 80 30)))",
            4900.0,
        ),
        (
            "shell inside hole, all vertices touch",
            "MULTIPOLYGON (((20 380, 420 380, 420 20, 20 20, 20 380), \
             (220 340, 180 240, 60 200, 180 160, 340 60, 240 220, 220 340)), \
             ((180 240, 180 160, 240 220, 180 240)))",
            128000.0,
        ),
        (
            "zero-area doubled ring",
            "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0, 10 0, 10 10, 0 10, 0 0))",
            0.0,
        ),
    ];
    for (name, text, want) in cases {
        let g = read_wkt(text).expect("case parses");
        let out = g.make_valid();
        assert!(is_valid(&out), "{name}: output invalid: {out:?}");
        let got = out.unsigned_area();
        let tol = 1e-6 * want.abs().max(1.0);
        assert!(
            (got - want).abs() <= tol,
            "{name}: area {got} against the even-odd {want}"
        );
    }
}

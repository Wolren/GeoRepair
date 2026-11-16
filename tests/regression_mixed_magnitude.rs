use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

// Fuzz-discovered (crash-eaab5472, CI 2026-08-04): a VALID ring whose
// 1-unit base sits at 1e15 while one vertex spikes to y=4.9e208.
#[test]
fn fuzz_valid_spike_ring_preserved() {
    let ring = vec![
        Coord { x: 1e15, y: 1e15 },
        Coord {
            x: 1e15 + 1.0,
            y: 1e15,
        },
        Coord {
            x: 1e15 + 1.0,
            y: 1e15 + 1.0,
        },
        Coord {
            x: 1e15,
            y: 4.919094327364069e208,
        },
    ];
    let poly = Polygon::new(LineString::new(ring), Vec::new());
    assert!(
        poly.validate().valid,
        "input must be valid per our validator"
    );
    for method in [PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
        let cfg = MakeValidConfig {
            poly_method: method,
            ..Default::default()
        };
        let out = poly.make_valid_with_config(&cfg);
        assert!(
            matches!(
                &out,
                geo::Geometry::Polygon(_) | geo::Geometry::MultiPolygon(_)
            ),
            "valid polygon was collapsed to {out:?} in mode {method:?}"
        );
        assert!(
            out.validate().valid,
            "repair output invalid in mode {method:?}"
        );
    }
}

// Fuzz-discovered deadly signal: a ring mixing denormals (7e-321), extreme magnitudes (5.5e+303, 1.9e+289), and normal coordinates.
#[test]
fn fuzz_mixed_magnitude_deadly_signal() {
    let mut ring = vec![
        Coord {
            x: 7.584e-321,
            y: 5.0,
        },
        Coord { x: 2.0, y: 1.5 },
        Coord {
            x: 2.0,
            y: 1.5000000018626451,
        },
        Coord { x: 0.0, y: 1.5 },
        Coord { x: 3.0, y: -1.0 },
        Coord { x: 4.5, y: -3.0 },
        Coord {
            x: 0.0,
            y: -5.486124073900545e303,
        },
        Coord {
            x: 2.0732734e-317,
            y: 6.214761e-317,
        },
        Coord {
            x: -1.9490628022880876e289,
            y: 1.5000000000004545,
        },
        Coord {
            x: -2.0,
            y: 1.5000000000727596,
        },
        Coord {
            x: 5.43230922487e-312,
            y: 5.0,
        },
    ];
    if ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    let poly = Polygon::new(LineString::new(ring), Vec::new());
    for method in [PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
        let cfg = MakeValidConfig {
            poly_method: method,
            ..Default::default()
        };
        let out = poly.make_valid_with_config(&cfg);
        assert!(
            out.validate().valid,
            "invalid output in mode {method:?}: {out:?}"
        );
    }
}

// The three sub-grid fuzz mutants (harness mut26, mut34, mut99): y reaches 1e208 and the
// sliver is 3.7e4 wide, below one step of the 1e8 snap grid at that extent. The repair
// cannot measure what it would lose, so it returns the input with the repeated vertex
// collapsed instead of an empty collection.
#[test]
fn sub_grid_mutants_keep_the_input() {
    let p = |x: f64, y: f64| Coord { x, y };
    let mut rings = vec![vec![
        p(1e15, 1e15),
        p(1e15, 1e15),
        p(1e15 + 1.0, 1e15),
        p(1e15 + 1.0, 1e15 + 1.0),
        p(1e15, 4.919094327364069e208),
        p(1e15, 1e15),
    ]];
    let sliver = vec![
        p(2.56e17, 2e15),
        p(3.6893297258671407e19, 4.918498623153493e208),
        p(3.6893297258671407e19, 4.918498623153493e208),
        p(3.6893297258671374e19, 4.918498623153493e208),
        p(2.56e17, 2e15),
    ];
    rings.push(sliver.clone());
    rings.push(sliver);
    for (k, ring) in rings.into_iter().enumerate() {
        let poly = Polygon::new(LineString::new(ring.clone()), Vec::new());
        let mut want: Vec<Coord<f64>> = Vec::new();
        for c in &ring {
            if want.last() != Some(c) {
                want.push(*c);
            }
        }
        for method in [PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
            let cfg = MakeValidConfig {
                poly_method: method,
                ..Default::default()
            };
            let out = poly.make_valid_with_config(&cfg);
            let geo::Geometry::Polygon(got) = &out else {
                panic!("mutant {k} in mode {method:?} came back as {out:?}");
            };
            assert_eq!(got.exterior().0, want, "mutant {k} in mode {method:?}");
            assert!(out.validate().valid, "mutant {k} in mode {method:?} is invalid");
        }
    }
}

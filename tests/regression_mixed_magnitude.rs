use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::io::wkt::read_wkt;
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
/// Documented limit (2026-11-17): the coordinates reach 1e257, past the 1e8 snap grid
/// and past a measurable even-odd area. The repair returns nothing; RustGIS keeps one
/// part. This test pins the empty output so a change is deliberate.
#[test]
fn extreme_span_self_intersection_returns_nothing() {
    let g = read_wkt(
        "POLYGON ((7.584e-321 0.0, 5.30239915e-315 1.1124672166448326e+212, \
         3.476364229858696e+210 1.1818214992282972e-124, 1.1818214992282972e-124 6.953531125181176e-309, \
         3.0003662109375 -1.0, 1.1818213291159835e-125 3.64228306305473e-310, \
         3.64228306217643e-310 1.148959759875365e-308, 4.556951262222753e-305 -4.0546456123406556e+257, \
         -2.0836291838344226e+256 -2.0836291838344226e+256, 2.7162230957734e-311 5.468344122175726e-304, \
         1.3920080192391e-309 5.0, 7.584e-321 0.0))",
    )
    .expect("the fuzz seed parses");
    let out = g.make_valid();
    assert!(
        matches!(&out, Geometry::GeometryCollection(gc) if gc.0.is_empty()),
        "documented empty output changed: {out:?}"
    );
}

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
        // A valid ring comes back unchanged; an invalid one below the snap grid comes
        // back with only its consecutive duplicates removed.
        let valid = poly.validate().valid;
        let mut want: Vec<Coord<f64>> = Vec::new();
        if valid {
            want = ring.clone();
        } else {
            for c in &ring {
                if want.last() != Some(c) {
                    want.push(*c);
                }
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
            assert!(
                out.validate().valid,
                "mutant {k} in mode {method:?} is invalid"
            );
        }
    }
}

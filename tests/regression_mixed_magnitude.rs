use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

// Fuzz-discovered (crash-eaab5472, CI 2026-08-04): a VALID ring whose
// 1-unit base sits at 1e15 while one vertex spikes to y=4.9e208. The
// bbox-scale degeneracy gates (make_valid pre-gate, strip bbox_ok,
// sub-ULP edge) compared extents against the max spread, so the spike
// dominated the base: every repair mode collapsed the valid polygon to
// GEOMETRYCOLLECTION EMPTY (and later to a LINESTRING after the pre-gate
// fix). GEOS IsValid keeps it; the preservation contract says valid in,
// polygonal out. All three modes must return the polygon unchanged.
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

// Fuzz-discovered deadly signal: a ring mixing denormals (7e-321),
// extreme magnitudes (5.5e+303, 1.9e+289), and normal coordinates.
// The repair pipeline must not crash (stack overflow / foreign assert)
// on any input, and must ship valid-or-empty output.
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
    // Mirror the fuzz target: append the closure vertex when unclosed.
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

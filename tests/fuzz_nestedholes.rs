//! Deterministic regression tests for specific failing proptest cases.
use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon, Winding};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig};

fn poly_from_coords(coords: Vec<[f64; 2]>) -> Polygon<f64> {
    let pts: Vec<Coord<f64>> = coords.iter().map(|&[x, y]| Coord { x, y }).collect();
    Polygon::new(LineString::new(pts), Vec::new())
}

#[test]
fn test_fuzz_nestedholes_seed1() {
    // Proptest seed from invariant_overlapping_multipolygon:
    let p1 = poly_from_coords(vec![
        [0.0, 10.70211681880726],
        [39.87837224853243, -13.667191282084643],
        [18.116970015191395, -0.14626862675891003],
        [0.0, 10.70211681880726],
    ]);
    let p2 = poly_from_coords(vec![
        [-0.510741842357813, -6.895507721309645],
        [31.970834623433966, -2.6925448774196097],
        [45.28763602741514, -8.553623836492713],
        [-41.03575166059883, 28.9699102108931],
        [-39.95533407589551, -45.357266508033995],
        [-31.028693473866316, 8.809751787664919],
        [-0.510741842357813, -6.895507721309645],
    ]);
    let mp = MultiPolygon::new(vec![p1, p2]);
    let result = mp.make_valid_with_config(&MakeValidConfig::default());
    eprintln!("result type: {:?}", std::mem::discriminant(&result));
    let validation = result.validate();
    eprintln!(
        "valid: {}, errors: {:?}",
        validation.valid, validation.errors
    );
    if !validation.valid {
        if let Geometry::MultiPolygon(mp) = &result {
            for (i, p) in mp.0.iter().enumerate() {
                eprintln!(
                    "  poly[{}]: nv={}, holes={}, area={:?}",
                    i,
                    p.exterior().0.len(),
                    p.interiors().len(),
                    p.exterior().winding_order()
                );
            }
        }
    }
    let valid = validation.valid;
    assert!(
        valid,
        "NestedHoles regression: {:?}",
        result.validate().errors
    );
}

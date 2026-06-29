use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

const INPUT: &[(f64, f64)] = &[
    (33.298685125309, 25.64285228568552),
    (16.056374168398353, 41.82073196346561),
    (5.2001056860635515, -1.4935771193319936),
    (40.0953181621632, 49.30127327981244),
    (-30.63143192804603, 22.339142189433932),
    (17.726542485814562, -29.738377616718996),
];

#[test]
fn diagnose_fuzz_6coord() {
    let mut coords: Vec<Coord<f64>> = INPUT.iter().map(|&(x, y)| Coord { x, y }).collect();
    if coords.first() != coords.last() {
        coords.push(coords[0]);
    }
    let poly = Polygon::new(LineString::new(coords), Vec::new());

    eprintln!(
        "=== DIAG_FIX_RING env: {:?} ===",
        std::env::var("DIAG_FIX_RING")
    );
    eprintln!("=== Poly validation before make_valid ===");
    let r = poly.validate();
    if r.valid {
        eprintln!("  VALID");
    } else {
        eprintln!("  INVALID: {:?}", r.errors);
    }

    for method in &[PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
        let config = MakeValidConfig {
            poly_method: *method,
            ..Default::default()
        };
        let result = poly.make_valid_with_config(&config);
        eprintln!("\n=== Method: {:?} ===", method);
        eprintln!("  Result type: {:?}", result);
        let r = result.validate();
        if r.valid {
            eprintln!("  Output VALID");
        } else {
            eprintln!("  Output INVALID: {:?}", r.errors);
        }
        match &result {
            geo::Geometry::Polygon(p) => {
                eprintln!("  Polygon ext: {} verts", p.exterior().0.len());
                for (i, h) in p.interiors().iter().enumerate() {
                    eprintln!("    Hole {}: {} verts", i, h.0.len());
                }
            }
            geo::Geometry::MultiPolygon(mp) => {
                eprintln!("  MultiPolygon with {} polys:", mp.0.len());
                for (i, p) in mp.0.iter().enumerate() {
                    eprintln!("    Poly {}: ext {} verts", i, p.exterior().0.len());
                }
            }
            geo::Geometry::GeometryCollection(gc) => {
                eprintln!("  GeometryCollection with {} items", gc.0.len());
            }
            _ => {
                eprintln!("  Other geometry");
            }
        }
    }

    // Now run with Structure with full diagnostics
    eprintln!("\n\n=== Structure with DIAG_FIX_RING ===");
    let config = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    poly.make_valid_with_config(&config);
}

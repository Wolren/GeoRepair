use geo::{Coord, Geometry, LineString, MultiPolygon, Polygon};
use geo::Area;
use geo_repair::validation::GeoValidation;
use geo_repair::MakeValid;

#[test]
fn diag() {
    let coords = vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: -293.0 },
        Coord { x: 236.0, y: -394.0 },
        Coord { x: -395.0, y: 0.0 },
        Coord { x: 840.0, y: -263.0 },
        Coord { x: -1.0, y: -446.0 },
        Coord { x: 0.0, y: 0.0 },
    ];
    let ring = LineString::new(coords.clone());
    let poly = Polygon::new(ring, Vec::new());
    println!("input valid: {}", poly.is_valid());
    let g: geo::Geometry<f64> = poly.into();
    let mv = geo_repair::MakeValid::make_valid(&g);
    let a = match &mv {
        Geometry::Polygon(p) => p.unsigned_area(),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(|p| p.unsigned_area()).sum(),
        Geometry::GeometryCollection(gc) => gc.0.iter().map(|x| match x {
            Geometry::Polygon(p) => p.unsigned_area(),
            _ => 0.0,
        }).sum(),
        _ => 0.0,
    };
    println!("make_valid: {a:.4}");
    // validate output
    match &mv {
        Geometry::Polygon(p) => {
            let v = GeoValidation::validate(p);
            println!("output errors: {:?}", v.errors.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>());
        }
        Geometry::MultiPolygon(mp) => {
            for (i, p) in mp.0.iter().enumerate() {
                let v = GeoValidation::validate(p);
                println!("output[{i}] errors: {:?}", v.errors.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>());
            }
        }
        _ => {}
    }
}

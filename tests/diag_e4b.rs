use geo::{Coord, Geometry, LineString, Polygon};
use geo::Area;
use geo_repair::validation::GeoValidation;
use geo_repair::MakeValid;

#[test]
fn diag() {
    let coords = vec![
        Coord { x: 63247462.032228455, y: 38653831.797141686 },
        Coord { x: 18152298.343705717, y: 27857720.36831153 },
        Coord { x: 0.0, y: -63200074.92363128 },
        Coord { x: 84150572.98717514, y: 71075092.36406943 },
        Coord { x: -69198718.0289094, y: -66575029.48790975 },
        Coord { x: 74413178.39423348, y: 97150137.01893745 },
    ];
    let mut ring = coords;
    if ring.len() >= 3 && ring.first() != ring.last() { ring.push(ring[0]); }
    let poly = Polygon::new(LineString::new(ring), Vec::new());
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
    println!("make_valid: {a:.4} type={}", geometry_type(&mv));
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

fn geometry_type(g: &Geometry<f64>) -> &'static str {
    match g {
        Geometry::Polygon(_) => "Polygon",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GC",
        _ => "other",
    }
}

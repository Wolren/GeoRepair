use geo::{Coord, Geometry, LineString, Polygon};
use geo::Area;
use geo_repair::core::{MakeValidConfig, PolyMethod};
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
    ];
    let mut ring = coords;
    if ring.len() >= 3 && ring.first() != ring.last() { ring.push(ring[0]); }
    let poly = Polygon::new(LineString::new(ring), Vec::new());
    let g: geo::Geometry<f64> = poly.into();
    let cfgs = [
        ("auto", MakeValidConfig::default()),
        ("structure", MakeValidConfig { poly_method: PolyMethod::Structure, ..Default::default() }),
        ("arrange", MakeValidConfig { poly_method: PolyMethod::Arrange, ..Default::default() }),
        ("structure+evenodd", MakeValidConfig { poly_method: PolyMethod::Structure, fill_rule: geo::algorithm::bool_ops::FillRule::EvenOdd, ..Default::default() }),
        ("auto_keep", MakeValidConfig { poly_method: PolyMethod::Auto, keep_collapsed: true, ..Default::default() }),
    ];
    for (name, cfg) in &cfgs {
        let mv = geo_repair::MakeValid::make_valid_with_config(&g, cfg);
        let a = match &mv {
            Geometry::Polygon(p) => p.unsigned_area(),
            Geometry::MultiPolygon(mp) => mp.0.iter().map(|p| p.unsigned_area()).sum(),
            Geometry::GeometryCollection(gc) => gc.0.iter().map(|x| match x {
                Geometry::Polygon(p) => p.unsigned_area(),
                _ => 0.0,
            }).sum(),
            _ => 0.0,
        };
        let mut errs: Vec<String> = Vec::new();
        match &mv {
            Geometry::Polygon(p) => {
                let v = GeoValidation::validate(p);
                errs.extend(v.errors.iter().map(|e| format!("{e:?}")));
            }
            Geometry::MultiPolygon(mp) => {
                for p in &mp.0 {
                    let v = GeoValidation::validate(p);
                    errs.extend(v.errors.iter().map(|e| format!("{e:?}")));
                }
            }
            _ => {}
        }
        println!("[{name}]: area={a:.4} errs={errs:?}");
    }
}

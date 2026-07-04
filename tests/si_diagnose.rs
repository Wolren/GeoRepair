//! Run each proptest regression seed through separate strategies.
use geo::{Coord, LineString, Polygon};
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fn to_poly(coords: &[[f64; 2]]) -> Polygon<f64> {
    let pts: Vec<Coord<f64>> = coords.iter().map(|&[x, y]| Coord { x, y }).collect();
    let mut ring = pts.clone();
    if ring.first() != ring.last() { ring.push(ring[0]); }
    Polygon::new(LineString::new(ring), Vec::new())
}

fn test_seed(name: &str, coords: &[[f64; 2]]) {
    let poly = to_poly(coords);
    for method in &[PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
        let cfg = MakeValidConfig { poly_method: *method, keep_collapsed: false, ..Default::default() };
        let result = poly.make_valid_with_config(&cfg);
        let v = result.validate();
        if !v.valid {
            eprintln!("FAIL|{name}|{method:?}|errors={:?}", v.errors);
        }
    }
}

fn test_mpoly(name: &str, polygons: &[&[[f64; 2]]]) {
    let polys: Vec<Polygon<f64>> = polygons.iter().map(|c| to_poly(c)).collect();
    use geo::MultiPolygon;
    let mp = MultiPolygon::new(polys);
    let g = geo::Geometry::MultiPolygon(mp);
    for method in &[PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
        let cfg = MakeValidConfig { poly_method: *method, keep_collapsed: false, ..Default::default() };
        let result = g.make_valid_with_config(&cfg);
        let v = result.validate();
        if !v.valid {
            eprintln!("FAIL|{name}|MP:{method:?}|errors={:?}", v.errors);
        }
    }
}

#[test]
fn diagnose_seeds() {
    test_seed("fig8", &[[0.0, 0.0], [50.0, 100.0], [100.0, 0.0], [50.0, -50.0]]);
    test_seed("collinear", &[[0.0, 0.0], [40.10194631387683, 0.0], [0.0, 0.0]]);
    test_seed("wrap", &[[0.0, 0.0], [1e-5, 0.0], [0.0, 1e-5]]);
    test_seed("seed1", &[[30.10815020588835, 36.966410520805205],
        [39.50119667611916, 96.7278626655685],
        [42.16959810646311, 0.0],
        [30.84209994951511, 72.47019033614485],
        [0.0, 98.37720168765794],
        [81.89269266553673, -97.81571023181081]]);
    test_seed("seed2", &[[-32.94925304356217, -37.4509724868373],
        [25.087850997208253, -29.87382634047737],
        [0.0, -48.64262720158944],
        [-40.61251938421724, -45.1172049629247],
        [-38.51974407936723, -13.433918287897887],
        [-16.8110711840133, -46.226614473001]]);
    test_seed("dup_coords", &[[0.0, 0.0], [0.0, 0.0], [0.0, -92.82263219157612], [0.0, -12.894268959376136]]);
    test_seed("large", &[[-77.11848127333822, -24.110460730681233],
        [-64.01818366691826, 48.7411801000996],
        [85.60424342929035, 36.83067927800398],
        [74.42093507035034, -50.8424755199042],
        [0.0, -62.25309887659038],
        [-23.102917007407697, -36.18841142195701],
        [23.683424831747818, -65.23242270566227],
        [0.0, -20.86218054324257],
        [-57.25662944655457, 0.0],
        [37.72516429296499, 0.0],
        [0.0, 38.0146216125035]]);
}

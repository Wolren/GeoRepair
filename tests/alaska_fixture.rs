//! Alaska SHP regression test.
//! Runs Structure fix on ALL Alaska polys and verifies every output
//! is GEOS-valid and area-preserving.
//! Requires `bench-geos` feature and the Alaska SHP at the expected path.

#![cfg(feature = "bench-geos")]

use geo::Polygon;
use geo_repair::io::load::{geo_area, signed_area};
#[cfg(feature = "load-shp")]
use geo_repair::io::load_shp;
use geo_repair::parallel::par_fix_polygon_batch;
use geo_repair::{MakeValidConfig, PolyMethod};
use geos::Geom;
use wkt::ToWkt;

#[test]
fn alaska_all_polys_geos_valid_and_area_preserved() {
    let polys = load_shp("benches/real_world/alaska.shp");
    eprintln!("Total Alaska polys loaded: {}", polys.len());

    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let all_refs: Vec<&Polygon<f64>> = polys.iter().collect();
    let results = par_fix_polygon_batch(&all_refs, &cfg);

    let mut total_invalid = 0usize;
    let mut area_failures = 0usize;
    let mut input_invalid = 0usize;

    for (i, (p, g)) in polys.iter().zip(results.iter()).enumerate() {
        let input_valid = geos::Geometry::new_from_wkt(&p.wkt_string())
            .ok()
            .map(|gg| gg.is_valid().unwrap_or(false))
            .unwrap_or(false);
        if !input_valid {
            input_invalid += 1;
        }

        let wkt = g.wkt_string();
        match geos::Geometry::new_from_wkt(&wkt) {
            Ok(gg) => {
                if !gg.is_valid().unwrap_or(false) {
                    total_invalid += 1;
                    let reason = gg.is_valid_reason().unwrap_or_else(|_| "unknown".into());
                    eprintln!("  Poly #{i}: still invalid after fix: {reason}");
                }
            }
            Err(e) => {
                total_invalid += 1;
                eprintln!("  Poly #{i}: WKT error after fix: {e}");
            }
        }

        let input_area = signed_area(&p.exterior().0);
        let output_area = geo_area(g);
        if input_area > 0.0 {
            let ratio = output_area / input_area;
            if !(0.5..=2.0).contains(&ratio) {
                area_failures += 1;
                eprintln!(
                    "  Poly #{i}: area ratio {:.4} (input={:.0}, output={:.0})",
                    ratio, input_area, output_area
                );
            }
        }
    }

    eprintln!(
        "Results: total={}, input_invalid={}, still_invalid_after_fix={}, area_failures={}",
        polys.len(),
        input_invalid,
        total_invalid,
        area_failures,
    );

    assert!(
        total_invalid == 0,
        "{total_invalid} polys still GEOS-invalid after Structure fix (out of {})",
        polys.len()
    );
    assert!(
        area_failures == 0,
        "{area_failures} polys with area ratio outside [0.5, 2.0]",
    );
}

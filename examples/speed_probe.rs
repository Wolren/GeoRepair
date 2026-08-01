//! Fast full-dataset speed probe: load all polys, parallel Structure fix,
//! print wall time. Skips the slow 2298-poly profile section of the bench.

use geo::Polygon;
use geo_repair::parallel::par_fix_polygon_batch;
use geo_repair::validation::GeoValidation;
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

#[cfg_attr(feature = "hotpath", hotpath::main(
    format = "json-pretty",
    output_path = "target/profiling/hotpath_report.json"
))]
fn main() {
    let polys = geo_repair::io::load_bin("benches/real_world/data_0.bin").expect("load");
    println!("loaded {} polys", polys.len());

    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let all: Vec<&Polygon<f64>> = polys.iter().collect();

    let t0 = std::time::Instant::now();
    let _results = par_fix_polygon_batch(&all, &cfg);
    let dt = t0.elapsed();
    println!(
        "FULL DATASET: {:.2}s for {} polys ({:.2} us/poly)",
        dt.as_secs_f64(),
        polys.len(),
        dt.as_secs_f64() / polys.len() as f64 * 1e6
    );

    // Valid-only sample (excludes the few huge invalid polys)
    let mut n = 0usize;
    let t1 = std::time::Instant::now();
    for p in polys.iter() {
        if p.is_valid() && p.exterior().0.len() <= 64 && p.interiors().is_empty() {
            let _ = p.make_valid_with_config(&cfg);
            n += 1;
            if n >= 200_000 {
                break;
            }
        }
    }
    let dt1 = t1.elapsed();
    println!(
        "VALID SMALL SAMPLE: {:.2}s for {} polys ({:.2} us/poly)",
        dt1.as_secs_f64(),
        n,
        dt1.as_secs_f64() / n as f64 * 1e6
    );
}

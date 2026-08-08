//! Fast full-dataset speed probe: load all polys, parallel Structure fix,
//! print wall time. Skips the slow 2298-poly profile section of the bench.

use geo_repair::parallel::par_fix_polygon_batch_owned;
use geo_repair::{MakeValidConfig, PolyMethod};

#[cfg_attr(
    feature = "hotpath",
    hotpath::main(
        format = "json-pretty",
        output_path = "target/profiling/hotpath_report.json"
    )
)]
fn main() {
    let polys = geo_repair::io::load_bin("benches/real_world/data_0.bin").expect("load");
    let n_total = polys.len();
    println!("loaded {n_total} polys");

    let cfg = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };

    let t0 = std::time::Instant::now();
    let _results = par_fix_polygon_batch_owned(polys, &cfg);
    let dt = t0.elapsed();
    println!(
        "FULL DATASET: {:.2}s for {} polys ({:.2} us/poly)",
        dt.as_secs_f64(),
        n_total,
        dt.as_secs_f64() / n_total as f64 * 1e6
    );
}

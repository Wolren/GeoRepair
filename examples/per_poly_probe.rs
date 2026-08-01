//! Sample every 5000th poly across the full dataset for cost distribution.
use geo_repair::MakeValid;

fn main() {
    let polys = geo_repair::io::load_bin("benches/real_world/data_0.bin").expect("load");
    let cfg = geo_repair::MakeValidConfig {
        poly_method: geo_repair::PolyMethod::Structure,
        ..Default::default()
    };
    let step = 5000usize;
    let mut total = 0.0;
    let mut slow: Vec<(f64, usize, usize)> = Vec::new(); // (dt, idx, verts)
    let t0 = std::time::Instant::now();
    let mut count = 0usize;
    for (i, p) in polys.iter().enumerate().step_by(step) {
        let t = std::time::Instant::now();
        let _ = p.make_valid_with_config(&cfg);
        let dt = t.elapsed().as_secs_f64();
        total += dt;
        count += 1;
        if dt > 0.001 {
            slow.push((dt, i, p.exterior().0.len()));
        }
    }
    let wall = t0.elapsed().as_secs_f64();
    println!("{count} samples, wall {wall:.2}s, total cpu {total:.2}s");
    slow.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (dt, i, v) in slow.iter().take(15) {
        println!("  {dt:.4}s idx={i} verts={v}");
    }
}

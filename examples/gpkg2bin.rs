//! Convert a GeoPackage into the flat polygon corpus the benches read.
//!
//! Load and write only: no validation, no repair, so the file reflects the stored coordinates.
//! Run: cargo run --release --features io-gpkg --example gpkg2bin -- in.gpkg out.bin

#[cfg(all(feature = "io-gpkg", not(target_arch = "wasm32")))]
fn main() {
    use geo::Geometry;
    use std::time::Instant;

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: gpkg2bin <input.gpkg> <output.bin>");
        std::process::exit(2);
    }

    let t = Instant::now();
    let geoms = geo_repair::io::gpkg::load_gpkg(&args[1]).expect("load gpkg");
    let load = t.elapsed().as_secs_f64();
    let rows = geoms.len();
    eprintln!("loaded {rows} geometries in {load:.2}s");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    let mut polys: Vec<geo::Polygon<f64>> = Vec::new();
    for g in geoms {
        match g {
            Geometry::Polygon(p) => polys.push(p),
            Geometry::MultiPolygon(mp) => polys.extend(mp.0),
            _ => {}
        }
    }
    let parts = polys.len();
    let coords: usize = polys.iter().map(|p| p.exterior().0.len()).sum();
    eprintln!("flattened to {parts} polygons, {coords} exterior coordinates");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    let t = Instant::now();
    geo_repair::io::write_bin(&args[2], &polys).expect("write bin");
    eprintln!("wrote {} in {:.2}s", args[2], t.elapsed().as_secs_f64());
}

#[cfg(not(all(feature = "io-gpkg", not(target_arch = "wasm32"))))]
fn main() {
    eprintln!("build with --features io-gpkg");
}

use std::time::Instant;

use geo_repair::io::{load_bin, read_wkb, write_wkb};
use geo_repair::{GeoValidation, MakeValid, MakeValidConfig, PolyMethod};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prog = args.first().map(|s| s.as_str()).unwrap_or("geo-repair-cli");

    if args.len() < 3 {
        eprintln!(
            "Usage: {prog} <input.wkb|input.bin> <output.wkb> [--method auto|structure|arrange]"
        );
        eprintln!();
        eprintln!("Supports:");
        eprintln!("  Input:  .wkb (concatenated WKB), .bin (custom binary format)");
        eprintln!("  Output: .wkb (concatenated WKB)");
        eprintln!();
        eprintln!("For other formats, use GEOS, GDAL, or QGIS to convert to/from WKB.");
        std::process::exit(1);
    }

    let input = &args[1];
    let output = &args[2];

    let method = args
        .iter()
        .position(|a| a == "--method")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("auto");

    let pm = match method.to_lowercase().as_str() {
        "arrange" | "arrangement" => PolyMethod::Arrange,
        "structure" => PolyMethod::Structure,
        _ => PolyMethod::Auto,
    };
    let config = MakeValidConfig {
        poly_method: pm,
        ..Default::default()
    };

    let ext = input.rsplit('.').next().unwrap_or("wkb");

    let t0 = Instant::now();
    eprintln!("Loading {input}...");

    let geoms = match ext {
        "bin" => {
            let polys = load_bin(input).unwrap_or_else(|e| {
                eprintln!("Error: failed to load {input}: {e}");
                std::process::exit(1);
            });
            polys
                .into_iter()
                .map(|p| geo::Geometry::Polygon(p))
                .collect::<Vec<_>>()
        }
        _ => {
            // Assume concatenated WKB
            let data = std::fs::read(input).unwrap_or_else(|e| {
                eprintln!("Error: failed to read {input}: {e}");
                std::process::exit(1);
            });
            let mut geoms = Vec::new();
            let mut offset = 0;
            while offset < data.len() {
                let geom = read_wkb(&data[offset..]).unwrap_or_else(|e| {
                    eprintln!("Error: WKB parse error at offset {offset}: {e}");
                    std::process::exit(1);
                });
                let consumed = geo_repair::io::estimate_wkb_size(&data[offset..])
                    .unwrap_or_else(|_| data.len() - offset);
                geoms.push(geom);
                offset += consumed;
            }
            geoms
        }
    };
    let load_time = t0.elapsed();
    let total = geoms.len();
    eprintln!("  Loaded {total} geometries in {load_time:.3?}");

    let t0 = Instant::now();
    eprintln!("Repairing...");
    let mut invalid = 0usize;
    let fixed: Vec<_> = geoms
        .into_iter()
        .map(|g| {
            if !g.is_valid() {
                invalid += 1;
            }
            g.make_valid_with_config(&config)
        })
        .collect();
    let fix_time = t0.elapsed();
    eprintln!("  Repaired {total} geometries ({invalid} invalid) in {fix_time:.3?}");

    let t0 = Instant::now();
    eprintln!("Writing {output}...");
    let mut out_buf = Vec::new();
    for g in &fixed {
        out_buf.extend_from_slice(&write_wkb(g));
    }
    std::fs::write(output, &out_buf).unwrap_or_else(|e| {
        eprintln!("Error: failed to write {output}: {e}");
        std::process::exit(1);
    });
    let write_time = t0.elapsed();
    eprintln!("  Wrote {} bytes in {write_time:.3?}", out_buf.len());

    let total_elapsed = load_time + fix_time + write_time;
    eprintln!("Total: {total_elapsed:.3?}");
}

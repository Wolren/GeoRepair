use std::time::Instant;

use geo_repair::io::{export_geometries, load_geometries};
use geo_repair::{GeoValidation, MakeValid, MakeValidConfig, PolyMethod};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prog = args.first().map(|s| s.as_str()).unwrap_or("geo-repair-cli");

    if args.len() < 3 {
        eprintln!("Usage: {prog} <input> <output> [--method auto|structure|arrange] [--machine]");
        eprintln!();
        eprintln!("Supported formats (auto-detected by extension):");
        eprintln!("  Input:  .shp, .geojson/.json, .wkt, .wkb, .bin, .csv, .gpkg");
        eprintln!("  Output: .shp, .geojson/.json, .wkt, .wkb, .csv");
        std::process::exit(1);
    }

    let input = &args[1];
    let output = &args[2];
    let machine = args.iter().any(|a| a == "--machine");

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

    let t0 = Instant::now();
    eprintln!("Loading {input}...");
    let geoms = load_geometries(input).unwrap_or_else(|e| {
        eprintln!("Error: failed to load {input}: {e}");
        std::process::exit(1);
    });
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
    export_geometries(&fixed, output).unwrap_or_else(|e| {
        eprintln!("Error: failed to write {output}: {e}");
        std::process::exit(1);
    });
    let write_time = t0.elapsed();
    let bytes = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    eprintln!("  Wrote {bytes} bytes in {write_time:.3?}");

    let total_elapsed = load_time + fix_time + write_time;
    if machine {
        let result = serde_json::json!({
            "total": total,
            "invalid": invalid,
            "bytes": bytes,
            "time_secs": total_elapsed.as_secs_f64(),
        });
        eprintln!("__RESULT__:{result}");
    }
}

//! Reads `id<TAB>wkt` and prints how RustGIS's own parser sees it, for the conformance run.
//!
//! Coordinates print as raw f64 bit patterns, so the comparison against the oracle's bytes is
//! exact and free of formatting differences.

use std::io::{self, BufRead};

use rustgis_data::geometry::Geometry;
use rustgis_geom::Coord;

fn bits(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn pt(c: &Coord) -> String {
    format!("{},{}", bits(c.x), bits(c.y))
}

fn pts(cs: &[Coord]) -> String {
    cs.iter().map(pt).collect::<Vec<_>>().join(" ")
}

fn shape(g: &Geometry) -> String {
    match g {
        Geometry::Point(c) => format!("P:{}", pt(c)),
        Geometry::MultiPoint(cs) => format!(
            "MP:{}",
            cs.iter().map(pt).collect::<Vec<_>>().join(";")
        ),
        Geometry::Line(parts) => format!(
            "L:{}",
            parts.iter().map(|p| pts(p)).collect::<Vec<_>>().join(";")
        ),
        Geometry::Polygon(parts) => format!(
            "PG:{}",
            parts
                .iter()
                .map(|rings| rings.iter().map(|r| pts(r)).collect::<Vec<_>>().join("|"))
                .collect::<Vec<_>>()
                .join(";")
        ),
    }
}

fn main() {
    for line in io::stdin().lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let (id, text) = match line.split_once('\t') {
            Some(p) => p,
            None => continue,
        };
        // A panic in the consumer must not abort the run either.
        let result = std::panic::catch_unwind(|| rustgis_data::interop::parse_wkt(text));
        match result {
            Ok(Ok((Some(g), _))) => println!("{}\tok\t{}", id, shape(&g)),
            Ok(Ok((None, _))) => println!("{}\tnone\t", id),
            Ok(Err(e)) => println!("{}\terr\t{}", id, e.replace('\t', " ")),
            Err(_) => println!("{}\tpanic\t", id),
        }
    }
}

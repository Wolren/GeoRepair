//! Reads the conformance corpus with the georust `wkt` and `wkb` crates and reports what they see.
//!
//! Mode flags: none reads WKT, `--wkb` reads WKB hex, `--wkt-out` re-emits parsed WKT.
//! Coordinates print as raw f64 bit patterns, matching the other probes.

use std::io::{self, BufRead};
use std::str::FromStr;

use geo_types::Geometry;
use wkt::ToWkt;

fn bits(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn pt(c: geo_types::Coord<f64>) -> String {
    format!("{},{}", bits(c.x), bits(c.y))
}

fn pts(cs: &[geo_types::Coord<f64>]) -> String {
    cs.iter().map(|c| pt(*c)).collect::<Vec<_>>().join(" ")
}

fn shape(g: &Geometry<f64>) -> String {
    match g {
        Geometry::Point(p) => format!("P:{}", pt(p.0)),
        Geometry::MultiPoint(mp) => format!(
            "MP:{}",
            mp.0.iter().map(|p| pt(p.0)).collect::<Vec<_>>().join(";")
        ),
        Geometry::Line(l) => format!("L:{} {}", pt(l.start), pt(l.end)),
        Geometry::LineString(ls) => format!("L:{}", pts(&ls.0)),
        Geometry::MultiLineString(mls) => format!(
            "L:{}",
            mls.0.iter().map(|l| pts(&l.0)).collect::<Vec<_>>().join(";")
        ),
        Geometry::Polygon(p) => format!(
            "PG:{}",
            std::iter::once(p.exterior())
                .chain(p.interiors().iter())
                .map(|r| pts(&r.0))
                .collect::<Vec<_>>()
                .join("|")
        ),
        Geometry::MultiPolygon(mp) => format!(
            "PG:{}",
            mp.0.iter()
                .map(|p| std::iter::once(p.exterior())
                    .chain(p.interiors().iter())
                    .map(|r| pts(&r.0))
                    .collect::<Vec<_>>()
                    .join("|"))
                .collect::<Vec<_>>()
                .join(";")
        ),
        Geometry::GeometryCollection(gc) => format!(
            "GC:{}",
            gc.0.iter().map(shape).collect::<Vec<_>>().join("&")
        ),
        Geometry::Rect(r) => shape(&Geometry::Polygon(r.to_polygon())),
        Geometry::Triangle(t) => shape(&Geometry::Polygon(t.to_polygon())),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let wkb_mode = args.iter().any(|a| a == "--wkb");
    let wkt_out = args.iter().any(|a| a == "--wkt-out");

    for line in io::stdin().lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let (id, payload) = match line.split_once('\t') {
            Some(p) => p,
            None => continue,
        };
        if wkb_mode {
            let bytes: Option<Vec<u8>> = from_hex(payload);
            match bytes {
                Some(b) => match wkb::reader::read_wkb(&b) {
                    Ok(g) => println!("{}\tok\t{:?}", id, g.geometry_type()),
                    Err(e) => println!("{}\terr\t{}", id, e.to_string().replace('\t', " ")),
                },
                None => println!("{}\terr\tbad hex", id),
            }
            continue;
        }
        // A panic in either crate must not abort the run.
        let parsed = std::panic::catch_unwind(|| {
            let w = wkt::Wkt::<f64>::from_str(payload)?;
            let g: Geometry<f64> = w.try_into().map_err(|e: wkt::conversion::Error| e.to_string())?;
            Ok::<_, String>(g)
        });
        match parsed {
            Ok(Ok(g)) => {
                if wkt_out {
                    println!("{}\tok\t{}", id, g.to_wkt());
                } else {
                    println!("{}\tok\t{}", id, shape(&g));
                }
            }
            Ok(Err(e)) => println!("{}\terr\t{}", id, e.replace('\t', " ")),
            Err(_) => println!("{}\tpanic\t", id),
        }
    }
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

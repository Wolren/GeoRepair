//! Diagnostic: check GEOS on idx=71527 (221K ext, 954 holes).
use geo::{Coord, LineString, Polygon};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use std::fs::File;
use std::io::Read;
use std::time::Instant;

fn read_f64(buf: &[u8], pos: &mut usize) -> f64 {
    let v = f64::from_le_bytes(buf[*pos..*pos + 8].try_into().unwrap());
    *pos += 8;
    v
}
fn read_u32(buf: &[u8], pos: &mut usize) -> u32 {
    let v = u32::from_le_bytes(buf[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    v
}
fn read_ring(buf: &[u8], pos: &mut usize) -> LineString<f64> {
    let n = read_u32(buf, pos) as usize;
    let mut coords = Vec::with_capacity(n);
    for _ in 0..n {
        coords.push(Coord {
            x: read_f64(buf, pos),
            y: read_f64(buf, pos),
        });
    }
    LineString::new(coords)
}
fn read_binary(path: &str) -> Vec<Polygon<f64>> {
    let mut buf = Vec::new();
    File::open(path).unwrap().read_to_end(&mut buf).unwrap();
    let mut pos = 0;
    let n_polys = read_u32(&buf, &mut pos) as usize;
    let mut polys = Vec::with_capacity(n_polys);
    for _ in 0..n_polys {
        let ext = read_ring(&buf, &mut pos);
        let n_holes = read_u32(&buf, &mut pos) as usize;
        let mut holes = Vec::with_capacity(n_holes);
        for _ in 0..n_holes {
            holes.push(read_ring(&buf, &mut pos));
        }
        polys.push(Polygon::new(ext, holes));
    }
    polys
}

fn main() {
    use geos::Geom;
    use std::io::Write;
    use wkt::ToWkt;

    let t0 = Instant::now();
    let polys = read_binary("benches/real_world/data_0.bin");
    eprintln!(
        "Loaded {} polys in {:.3}s",
        polys.len(),
        t0.elapsed().as_secs_f64()
    );

    let idx = 71527;
    let poly = &polys[idx];
    let ext_n = poly.exterior().0.len();
    let n_holes = poly.interiors().len();
    let tot: usize = ext_n + poly.interiors().iter().map(|h| h.0.len()).sum::<usize>();
    eprintln!("idx={idx}: ext={ext_n} holes={n_holes} tot={tot}");

    // Time GEOS: WKT generation
    eprint!("  WKT gen...");
    std::io::stderr().flush().ok();
    let t0 = Instant::now();
    let wkt = poly.wkt_string();
    eprintln!(" {:.4}s", t0.elapsed().as_secs_f64());

    // Time GEOS: parse + make_valid
    eprint!("  GEOS parse+make_valid...");
    std::io::stderr().flush().ok();
    let t0 = Instant::now();
    match geos::Geometry::new_from_wkt(&wkt) {
        Ok(g) => {
            let _ = g.make_valid();
            eprintln!(" {:.4}s", t0.elapsed().as_secs_f64());
        }
        Err(e) => eprintln!(" err: {e}"),
    }
}

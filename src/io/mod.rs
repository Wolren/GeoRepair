use geo::{Coord, Geometry, Polygon};

pub mod binary;
pub mod wkb;

pub use binary::{load_bin, load_bin_stream};
pub use wkb::{estimate_wkb_size, read_wkb, read_wkb_concat, write_wkb};

// ---------------------------------------------------------------------------
// Geometry area utilities (used by tests and benchmarks)
// ---------------------------------------------------------------------------

pub fn signed_area(ring: &[Coord<f64>]) -> f64 {
    let mut s = 0.0;
    for w in ring.windows(2) {
        s += w[0].x * w[1].y - w[1].x * w[0].y;
    }
    s / 2.0
}

pub fn polygon_area(p: &Polygon<f64>) -> f64 {
    signed_area(&p.exterior().0).abs()
}

pub fn geo_area(g: &Geometry<f64>) -> f64 {
    match g {
        Geometry::Polygon(p) => polygon_area(p),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(polygon_area).sum(),
        _ => 0.0,
    }
}

pub fn count_sub_polys(g: &Geometry<f64>) -> usize {
    match g {
        Geometry::Polygon(_) => 1,
        Geometry::MultiPolygon(mp) => mp.0.len(),
        _ => 0,
    }
}

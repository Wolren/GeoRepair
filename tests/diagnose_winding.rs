use geo::algorithm::Winding;
use geo::{Coord, LineString};

fn main() {
    let coords = vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: 10.0 },
        Coord { x: 10.0, y: 10.0 },
        Coord { x: 10.0, y: 0.0 },
        Coord { x: 0.0, y: 0.0 },
    ];
    let mut ring = LineString::new(coords);
    let wo = ring.winding_order();
    eprintln!("winding_order: {wo:?}");

    ring.make_ccw_winding();
    let wo2 = ring.winding_order();
    eprintln!("after make_ccw_winding: {wo2:?}");
    eprintln!("coords: {:?}", ring.0);
}

use geo::winding_order::{Winding, WindingOrder};
use geo::{Coord, LineString};

#[test]
fn test_winding_diagnose() {
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
    assert_eq!(wo2, Some(WindingOrder::CounterClockwise));
}

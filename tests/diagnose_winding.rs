use geo::winding_order::WindingOrder;
use geo::Winding;
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
    assert_eq!(wo, Some(WindingOrder::Clockwise));

    ring.make_ccw_winding();
    let wo2 = ring.winding_order();
    assert_eq!(wo2, Some(WindingOrder::CounterClockwise));
}

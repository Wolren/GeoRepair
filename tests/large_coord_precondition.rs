//! Preconditioning contract (2026-09-18): uniform-magnitude large or small
//! inputs that the snap guard routes away get a bit-exact similarity
//! transform, the normal repair path runs at that magnitude, and the
//! result maps back. Pass-through cases must return the input's own
//! coordinates bit-exactly; repaired cases must be valid and area-honest;
//! mixed-magnitude sub-grid features keep the legacy route.

use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::make_valid::{is_valid_with_geo, make_valid_owned};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};

fn ring(n: usize, r: f64, scale: f64, shift: f64) -> Polygon<f64> {
    let mut coords = Vec::with_capacity(n);
    for i in 0..n - 1 {
        let a = 2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64;
        coords.push(Coord {
            x: (r * a.cos() + 500.0) * scale + shift,
            y: (r * a.sin() + 500.0) * scale + shift,
        });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), Vec::new())
}

fn bowtie(scale: f64, shift: f64) -> Polygon<f64> {
    Polygon::new(
        LineString::new(vec![
            Coord { x: shift, y: shift },
            Coord {
                x: scale + shift,
                y: scale + shift,
            },
            Coord {
                x: scale + shift,
                y: shift,
            },
            Coord {
                x: shift,
                y: scale + shift,
            },
            Coord { x: shift, y: shift },
        ]),
        Vec::new(),
    )
}

fn sorted_coords(p: &Polygon<f64>) -> Vec<(u64, u64)> {
    let mut v: Vec<(u64, u64)> = p
        .exterior()
        .0
        .iter()
        .map(|c| (c.x.to_bits(), c.y.to_bits()))
        .collect();
    v.sort_unstable();
    v
}

fn polygon_of(g: &Geometry<f64>) -> &Polygon<f64> {
    if let Geometry::Polygon(p) = g {
        p
    } else {
        panic!("expected a Polygon, got {g:?}");
    }
}

fn both_configs() -> [MakeValidConfig; 2] {
    [
        MakeValidConfig {
            poly_method: PolyMethod::Structure,
            ..Default::default()
        },
        MakeValidConfig::default(),
    ]
}

#[test]
fn valid_large_ring_roundtrips_exactly() {
    let p = ring(256, 100.0, 1e10, 1e12);
    for cfg in both_configs() {
        let g = p.make_valid_with_config(&cfg);
        assert!(is_valid_with_geo(&g), "fast-path cert under {cfg:?}");
        assert_eq!(
            sorted_coords(polygon_of(&g)),
            sorted_coords(&p),
            "a certified pass-through must return the input coordinates bit-exactly"
        );
        let g_owned = make_valid_owned(p.clone(), &cfg);
        assert!(is_valid_with_geo(&g_owned));
        assert_eq!(sorted_coords(polygon_of(&g_owned)), sorted_coords(&p));
    }
}

#[test]
fn valid_tiny_ring_roundtrips_exactly() {
    let p = ring(256, 100.0, 1e-12, 0.0);
    for cfg in both_configs() {
        let g = p.make_valid_with_config(&cfg);
        assert!(is_valid_with_geo(&g), "fast-path cert under {cfg:?}");
        assert_eq!(sorted_coords(polygon_of(&g)), sorted_coords(&p));
    }
}

#[test]
fn bowtie_at_large_magnitude_repairs_valid_and_area_honest() {
    let scale = 1e10;
    let p = bowtie(scale, 1e12);
    for cfg in both_configs() {
        let g = p.make_valid_with_config(&cfg);
        assert!(is_valid_with_geo(&g), "repair output must be valid: {g:?}");
        let mp_area = match &g {
            Geometry::MultiPolygon(mp) => {
                mp.0.iter()
                    .map(|q| {
                        use geo::Area;
                        q.unsigned_area()
                    })
                    .sum::<f64>()
            }
            Geometry::Polygon(q) => {
                use geo::Area;
                q.unsigned_area()
            }
            other => panic!("unexpected repair output {other:?}"),
        };
        let expected = scale * scale / 2.0;
        assert!(
            (mp_area - expected).abs() / expected < 1e-6,
            "area {mp_area} vs expected {expected} under {cfg:?}"
        );
    }
}

#[test]
fn sub_grid_feature_keeps_a_valid_result() {
    // A genuine micro feature (1e-12 x delta) inside a 1e7 span cannot
    // survive the snap grid; the preconditioning transform must decline
    // and the legacy route must still produce a valid geometry.
    let p = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1e-12, y: 1e-12 },
            Coord { x: 1e7, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    for cfg in both_configs() {
        let g = p.make_valid_with_config(&cfg);
        assert!(is_valid_with_geo(&g), "under {cfg:?}: {g:?}");
    }
}

#[test]
fn nan_at_large_magnitude_still_takes_the_nan_path() {
    let mut p = ring(128, 100.0, 1e10, 1e12);
    p.exterior_mut(|ls| ls.0[40].x = f64::NAN);
    for cfg in both_configs() {
        let g = p.make_valid_with_config(&cfg);
        assert!(is_valid_with_geo(&g), "under {cfg:?}");
        match &g {
            Geometry::Polygon(q) => assert!(q.exterior().0.len() >= 4),
            Geometry::MultiPolygon(mp) => assert!(!mp.0.is_empty()),
            other => panic!("unexpected NaN-path output {other:?}"),
        }
    }
}

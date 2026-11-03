use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiPoint, MultiPolygon, Point, Polygon, Rect,
};
use geo_repair::host::{HostOptions, host_clean, to_host, to_host_ewkt, to_host_with, to_host_wkb};
use geo_repair::validation::GeoValidation;

fn ring(coords: &[(f64, f64)]) -> LineString<f64> {
    LineString::from(coords.to_vec())
}

fn square() -> Polygon<f64> {
    Polygon::new(
        ring(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]),
        vec![],
    )
}

#[test]
fn a_collection_flattens_to_its_top_dimension() {
    let gc = GeometryCollection::new_from(vec![
        Geometry::Point(Point::new(1.0, 2.0)),
        Geometry::LineString(LineString::from(vec![(0.0, 0.0), (1.0, 1.0)])),
    ]);
    let (out, report) = to_host(&Geometry::GeometryCollection(gc));
    assert_eq!(report.flattened_collections, 1);
    assert!(matches!(out, Some(Geometry::LineString(_))));
    assert_eq!(host_clean(out.as_ref().expect("a line survives")), Ok(()));
}

#[test]
fn the_type_follows_the_survivors() {
    // One dead member, one live member: a host gets a Point, not a single-member MultiPoint.
    let gc = GeometryCollection::new_from(vec![
        Geometry::Point(Point::new(f64::NAN, f64::NAN)),
        Geometry::Point(Point::new(1.0, 2.0)),
    ]);
    let (out, report) = to_host(&Geometry::GeometryCollection(gc));
    assert_eq!(report.dropped_nonfinite, 1);
    assert_eq!(out, Some(Geometry::Point(Point::new(1.0, 2.0))));
}

#[test]
fn nothing_usable_gives_none() {
    let (out, report) = to_host(&Geometry::Point(Point::new(f64::INFINITY, 0.0)));
    assert!(out.is_none());
    assert_eq!(report.dropped_nonfinite, 1);
}

#[test]
fn an_open_ring_is_closed_and_the_shell_kept() {
    let p = Polygon::new(ring(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]), vec![]);
    match to_host(&Geometry::Polygon(p))
        .0
        .expect("a triangle survives")
    {
        Geometry::Polygon(p) => {
            assert_eq!(p.exterior().0.len(), 4);
            assert_eq!(p.exterior().0[0], p.exterior().0[3]);
        }
        other => panic!("expected a polygon, got {other:?}"),
    }
}

#[test]
fn a_non_finite_hole_is_dropped_and_the_shell_survives() {
    let p = Polygon::new(
        square().exterior().clone(),
        vec![ring(&[(1.0, 1.0), (f64::NAN, 2.0), (3.0, 3.0)])],
    );
    let (out, report) = to_host(&Geometry::Polygon(p));
    assert_eq!(report.dropped_nonfinite, 1);
    match out.expect("the shell survives") {
        Geometry::Polygon(p) => assert!(p.interiors().is_empty()),
        other => panic!("expected a polygon, got {other:?}"),
    }
}

#[test]
fn a_two_coordinate_ring_costs_the_polygon() {
    let p = Polygon::new(ring(&[(0.0, 0.0), (1.0, 1.0)]), vec![]);
    let (out, report) = to_host(&Geometry::Polygon(p));
    assert!(out.is_none());
    assert_eq!(report.dropped_degenerate, 1);
}

#[test]
fn an_empty_polygon_part_is_dropped_and_the_rest_survives() {
    let empty = Polygon::new(ring(&[]), vec![]);
    let mp = MultiPolygon::new(vec![empty, square()]);
    let (out, report) = to_host(&Geometry::MultiPolygon(mp));
    assert_eq!(report.dropped_degenerate, 1);
    assert!(matches!(out, Some(Geometry::Polygon(_))));
}

#[test]
fn a_rect_becomes_a_polygon() {
    let r = Rect::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 });
    let (out, _) = to_host(&Geometry::Rect(r));
    assert!(matches!(out, Some(Geometry::Polygon(_))));
}

#[test]
fn a_multipoint_keeps_only_finite_members() {
    let mp = MultiPoint::new(vec![
        Point::new(f64::NAN, 0.0),
        Point::new(1.0, 1.0),
        Point::new(2.0, 2.0),
    ]);
    let (out, report) = to_host(&Geometry::MultiPoint(mp));
    assert_eq!(report.dropped_nonfinite, 1);
    match out.expect("two members survive") {
        Geometry::MultiPoint(mp) => assert_eq!(mp.0.len(), 2),
        other => panic!("expected a multipoint, got {other:?}"),
    }
}

/// Deterministic pseudo-random corpus: the invariants must hold for every shape, not for the
/// examples a person thought to write down.
#[test]
fn every_generated_shape_obeys_the_host_contract() {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let pick = |n: u64, next: &mut dyn FnMut() -> u64| (next() % n) as usize;

    let mut seen_none = 0;
    for _ in 0..500 {
        let mut parts: Vec<Geometry<f64>> = Vec::new();
        for _ in 0..(1 + pick(3, &mut next)) {
            let bad = pick(4, &mut next) == 0;
            let v = |next: &mut dyn FnMut() -> u64| {
                let r = (next() % 1000) as f64 / 10.0;
                if bad && pick(2, next) == 0 {
                    if pick(2, next) == 0 {
                        f64::NAN
                    } else {
                        f64::INFINITY
                    }
                } else {
                    r
                }
            };
            let shape = pick(4, &mut next);
            let g = match shape {
                0 => Geometry::Point(Point::new(v(&mut next), v(&mut next))),
                1 => {
                    let n = pick(4, &mut next);
                    Geometry::LineString(LineString::from(
                        (0..n)
                            .map(|_| (v(&mut next), v(&mut next)))
                            .collect::<Vec<_>>(),
                    ))
                }
                _ => {
                    let n = 3 + pick(4, &mut next);
                    let mut coords: Vec<(f64, f64)> =
                        (0..n).map(|_| (v(&mut next), v(&mut next))).collect();
                    if pick(2, &mut next) == 0 {
                        coords.push(coords[0]);
                    }
                    Geometry::Polygon(Polygon::new(LineString::from(coords), vec![]))
                }
            };
            parts.push(g);
        }
        let input = Geometry::GeometryCollection(GeometryCollection::new_from(parts));
        let (out, _report) = to_host(&input);
        match out {
            None => seen_none += 1,
            Some(g) => {
                assert_eq!(host_clean(&g), Ok(()), "broken output for {g:?}");
                assert!(!matches!(g, Geometry::GeometryCollection(_)));
                let (again, again_report) = to_host(&g);
                assert_eq!(again, Some(g.clone()), "not idempotent");
                assert_eq!(again_report.flattened_collections, 0);
                assert_eq!(again_report.dropped_nonfinite, 0);
                assert_eq!(again_report.dropped_degenerate, 0);
            }
        }
    }
    assert!(
        seen_none < 500,
        "every generated shape was empty, the test proves nothing"
    );
}

fn bowtie() -> Polygon<f64> {
    Polygon::new(
        ring(&[(0.0, 0.0), (1.0, 1.0), (1.0, 0.0), (0.0, 1.0), (0.0, 0.0)]),
        vec![],
    )
}

#[test]
fn reduce_alone_leaves_a_self_intersection_alone() {
    let (out, report) = to_host_with(&Geometry::Polygon(bowtie()), HostOptions::REDUCE);
    assert_eq!(report.repaired, 0);
    match out.expect("still a polygon") {
        Geometry::Polygon(p) => assert!(!p.is_valid(), "the bowtie must stay broken"),
        other => panic!("expected a polygon, got {other:?}"),
    }
}

#[test]
fn repair_mode_returns_valid_parts() {
    let (out, report) = to_host_with(&Geometry::Polygon(bowtie()), HostOptions::REPAIR);
    assert_eq!(report.repaired, 1);
    let out = out.expect("repair keeps the area");
    assert_eq!(host_clean(&out), Ok(()));
    match out {
        Geometry::Polygon(p) => assert!(p.is_valid()),
        Geometry::MultiPolygon(mp) => {
            assert!(mp.0.len() >= 2, "a bowtie breaks into two lobes");
            assert!(mp.0.iter().all(|p| p.is_valid()));
        }
        other => panic!("expected polygonal output, got {other:?}"),
    }
}

#[test]
fn repair_mode_does_not_touch_a_valid_polygon() {
    let square = square();
    let (out, report) = to_host_with(&Geometry::Polygon(square.clone()), HostOptions::REPAIR);
    assert_eq!(report.repaired, 0);
    assert_eq!(out, Some(Geometry::Polygon(square)));
}

#[test]
fn a_host_can_be_fed_bytes_and_get_repair() {
    let bytes = geo_repair::io::wkb::write_wkb(&Geometry::Polygon(bowtie()));
    let (out, report) = to_host_wkb(&bytes, HostOptions::REPAIR).expect("bytes read");
    assert_eq!(report.repaired, 1);
    let out = out.expect("repair keeps the area");
    assert_eq!(host_clean(&out), Ok(()));
    let valid = match &out {
        Geometry::Polygon(p) => p.is_valid(),
        Geometry::MultiPolygon(mp) => mp.0.iter().all(|p| p.is_valid()),
        _ => false,
    };
    assert!(valid, "delivered geometry is still invalid");
}

#[test]
fn a_host_can_be_fed_ewkt_and_get_a_srid_free_shape() {
    let (out, _) = to_host_ewkt("SRID=4326;POINT (1 2)", HostOptions::REDUCE).expect("ewkt read");
    assert_eq!(out, Some(Geometry::Point(Point::new(1.0, 2.0))));
}

fn is_valid_any(g: &Geometry<f64>) -> bool {
    match g {
        Geometry::Point(p) => p.is_valid(),
        Geometry::MultiPoint(p) => p.is_valid(),
        Geometry::LineString(p) => p.is_valid(),
        Geometry::MultiLineString(p) => p.is_valid(),
        Geometry::Polygon(p) => p.is_valid(),
        Geometry::MultiPolygon(p) => p.is_valid(),
        _ => false,
    }
}

#[test]
fn repair_mode_fixes_a_self_crossing_line() {
    let line = LineString::from(vec![(0.0, 0.0), (2.0, 2.0), (2.0, 0.0), (0.0, 2.0)]);
    let (raw, _) = to_host_with(&Geometry::LineString(line.clone()), HostOptions::REDUCE);
    assert!(
        !is_valid_any(&raw.expect("line survives reduce")),
        "input must be broken"
    );

    let (out, report) = to_host_with(&Geometry::LineString(line), HostOptions::REPAIR);
    assert_eq!(report.repaired, 1);
    let out = out.expect("a crossing line still has length");
    assert_eq!(host_clean(&out), Ok(()));
    assert!(
        is_valid_any(&out),
        "repaired line is still invalid: {out:?}"
    );
    assert!(matches!(
        out,
        Geometry::LineString(_) | Geometry::MultiLineString(_)
    ));
}

#[test]
fn repair_mode_dedupes_repeated_points() {
    let mp = MultiPoint::new(vec![Point::new(1.0, 1.0), Point::new(1.0, 1.0)]);
    let (raw, _) = to_host_with(&Geometry::MultiPoint(mp.clone()), HostOptions::REDUCE);
    assert!(!is_valid_any(&raw.expect("points survive reduce")));

    let (out, report) = to_host_with(&Geometry::MultiPoint(mp), HostOptions::REPAIR);
    assert_eq!(report.repaired, 1);
    let out = out.expect("one point survives dedupe");
    assert!(
        is_valid_any(&out),
        "repaired multipoint is still invalid: {out:?}"
    );
}

#[test]
fn repair_mode_fixes_overlapping_polygons() {
    // Each square is valid on its own; the pair is not, so validity belongs to the collection.
    let a = Polygon::new(
        ring(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (0.0, 0.0)]),
        vec![],
    );
    let b = Polygon::new(
        ring(&[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0), (1.0, 1.0)]),
        vec![],
    );
    let mp = MultiPolygon::new(vec![a.clone(), b.clone()]);
    assert!(a.is_valid() && b.is_valid());
    let (raw, _) = to_host_with(&Geometry::MultiPolygon(mp.clone()), HostOptions::REDUCE);
    assert!(!is_valid_any(&raw.expect("both survive reduce")));

    let (out, report) = to_host_with(&Geometry::MultiPolygon(mp), HostOptions::REPAIR);
    assert_eq!(report.repaired, 1);
    let out = out.expect("overlap keeps area");
    assert_eq!(host_clean(&out), Ok(()));
    assert!(
        is_valid_any(&out),
        "repaired multipolygon is still invalid: {out:?}"
    );
}

#[test]
fn repair_mode_fixes_crossing_members_of_a_multi_line() {
    // RustGIS stores every line as a vector of lines, so crossing members arrive as one shape.
    let mls = geo::MultiLineString::new(vec![
        LineString::from(vec![(0.0, 0.0), (2.0, 2.0)]),
        LineString::from(vec![(2.0, 0.0), (0.0, 2.0)]),
    ]);
    let (raw, _) = to_host_with(&Geometry::MultiLineString(mls.clone()), HostOptions::REDUCE);
    assert!(!is_valid_any(&raw.expect("lines survive reduce")));

    let (out, report) = to_host_with(&Geometry::MultiLineString(mls), HostOptions::REPAIR);
    assert_eq!(report.repaired, 1);
    let out = out.expect("crossing lines keep length");
    assert_eq!(host_clean(&out), Ok(()));
    assert!(is_valid_any(&out), "crossing pair still invalid: {out:?}");
}

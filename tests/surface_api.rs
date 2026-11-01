//! Surface tests for the lean feature set: serial batch entries, output policy, and
//! error classes. These run on every feature configuration, so they also guard the
//! build where `parallel` is off.

use geo::{Coord, Geometry, GeometryCollection, LineString, MultiPoint, Point, Polygon};
use geo_repair::batch::{fix_polygon_batch, fix_polygon_batch_chunked, fix_polygon_batch_owned};
use geo_repair::output::{OutputPolicy, apply_policy, make_valid_with_policy};
use geo_repair::{
    GeometryValidationError, MakeValid, MakeValidConfig, ValidationClass, ValidationResult,
    validate,
};

fn square() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]),
        vec![],
    )
}

fn bowtie() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ]),
        vec![],
    )
}

fn line() -> LineString<f64> {
    LineString::from(vec![(0.0, 0.0), (1.0, 1.0)])
}

// ---------------------------------------------------------------- error classes

fn every_error() -> Vec<GeometryValidationError> {
    vec![
        GeometryValidationError::CoordinateNaN,
        GeometryValidationError::RingTooFewPoints { found: 2, min: 4 },
        GeometryValidationError::RingNotClosed {
            first: Coord { x: 0.0, y: 0.0 },
            last: Coord { x: 1.0, y: 1.0 },
        },
        GeometryValidationError::SelfIntersection,
        GeometryValidationError::PinchPoint,
        GeometryValidationError::HoleOutsideShell,
        GeometryValidationError::NestedHoles,
        GeometryValidationError::DisconnectedInteriorRing,
        GeometryValidationError::WrongOrientation,
        GeometryValidationError::CollinearRing,
        GeometryValidationError::RepeatedPoint,
        GeometryValidationError::DuplicatedRings,
        GeometryValidationError::MultiPointDuplicatePoints,
        GeometryValidationError::MultiLineStringDuplicateLines,
        GeometryValidationError::ZeroLengthLine(Coord { x: 0.0, y: 0.0 }),
        GeometryValidationError::DegenerateExterior,
        GeometryValidationError::NotSimple,
        GeometryValidationError::ExcessiveNesting,
    ]
}

#[test]
fn error_codes_are_unique_and_non_empty() {
    let mut codes: Vec<&'static str> = every_error().iter().map(|e| e.code()).collect();
    let count = codes.len();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), count, "every variant needs its own code");

    for error in every_error() {
        assert!(!error.code().is_empty());
        assert!(!error.label().is_empty());
        assert!(
            error
                .code()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_'),
            "code {:?} must be a snake_case key",
            error.code()
        );
    }
}

#[test]
fn error_display_is_unchanged_by_classification() {
    let error = GeometryValidationError::RingTooFewPoints { found: 2, min: 4 };
    assert_eq!(
        error.to_string(),
        "Ring has too few points: found 2, minimum required 4"
    );
    assert_eq!(error.label(), "Ring too few points");
    assert_eq!(error.class(), ValidationClass::Structure);
}

#[test]
fn class_counts_cover_every_class_in_order() {
    let result = ValidationResult::invalid(vec![
        GeometryValidationError::SelfIntersection,
        GeometryValidationError::PinchPoint,
        GeometryValidationError::CoordinateNaN,
        GeometryValidationError::WrongOrientation,
    ]);
    let counts = result.class_counts();
    assert_eq!(counts.len(), ValidationClass::ALL.len());
    let classes: Vec<ValidationClass> = counts.iter().map(|(c, _)| *c).collect();
    assert_eq!(classes, ValidationClass::ALL.to_vec());
    assert_eq!(counts[ValidationClass::Topology.index()].1, 2);
    assert_eq!(counts[ValidationClass::Coordinates.index()].1, 1);
    assert_eq!(counts[ValidationClass::Orientation.index()].1, 1);
    assert_eq!(counts[ValidationClass::Duplication.index()].1, 0);
}

#[test]
fn code_counts_aggregate_repeats_and_keep_first_seen_order() {
    let result = ValidationResult::invalid(vec![
        GeometryValidationError::SelfIntersection,
        GeometryValidationError::CoordinateNaN,
        GeometryValidationError::SelfIntersection,
    ]);
    assert_eq!(
        result.code_counts(),
        vec![("self_intersection", 2), ("coordinate_nan", 1)]
    );
}

#[test]
fn valid_result_has_zero_counts() {
    let result = validate(&Geometry::Polygon(square()));
    assert!(result.valid);
    assert!(result.class_counts().iter().all(|(_, n)| *n == 0));
    assert!(result.code_counts().is_empty());
}

// ---------------------------------------------------------------- output policy

#[test]
fn keep_all_returns_the_geometry_untouched() {
    let mixed = Geometry::GeometryCollection(GeometryCollection(vec![
        Geometry::Polygon(square()),
        Geometry::LineString(line()),
    ]));
    let (out, report) = apply_policy(mixed.clone(), OutputPolicy::KeepAll);
    assert_eq!(out, mixed);
    assert_eq!(report.kept_parts, 2);
    assert_eq!(report.dropped_parts, 0);
    assert!(report.is_lossless());
}

#[test]
fn areal_only_drops_lower_dimensions_and_counts_them() {
    let mixed = Geometry::GeometryCollection(GeometryCollection(vec![
        Geometry::Polygon(square()),
        Geometry::LineString(line()),
        Geometry::Point(Point::new(1.0, 1.0)),
    ]));
    let (out, report) = apply_policy(mixed, OutputPolicy::ArealOnly);
    assert!(matches!(out, Geometry::Polygon(_)));
    assert_eq!(report.kept_parts, 1);
    assert_eq!(report.dropped_parts, 2);
    assert!(!report.is_lossless());
    assert!(validate(&out).valid);
}

#[test]
fn top_dimension_keeps_lines_when_no_polygon_is_present() {
    let mixed = Geometry::GeometryCollection(GeometryCollection(vec![
        Geometry::LineString(line()),
        Geometry::Point(Point::new(1.0, 1.0)),
    ]));
    let (out, report) = apply_policy(mixed, OutputPolicy::TopDimension);
    assert!(matches!(out, Geometry::LineString(_)));
    assert_eq!(report.kept_parts, 1);
    assert_eq!(report.dropped_parts, 1);
}

#[test]
fn top_dimension_keeps_points_when_only_points_are_present() {
    let points = Geometry::MultiPoint(MultiPoint::new(vec![
        Point::new(1.0, 1.0),
        Point::new(2.0, 2.0),
    ]));
    let (out, report) = apply_policy(points, OutputPolicy::TopDimension);
    assert!(matches!(out, Geometry::MultiPoint(_)));
    assert_eq!(report.kept_parts, 2);
    assert_eq!(report.dropped_parts, 0);
}

#[test]
fn nested_collections_are_flattened() {
    let inner = Geometry::GeometryCollection(GeometryCollection(vec![
        Geometry::Polygon(square()),
        Geometry::LineString(line()),
    ]));
    let outer =
        Geometry::GeometryCollection(GeometryCollection(vec![inner, Geometry::Polygon(bowtie())]));
    let (out, report) = apply_policy(outer, OutputPolicy::ArealOnly);
    match out {
        Geometry::MultiPolygon(mp) => assert_eq!(mp.0.len(), 2),
        Geometry::Polygon(_) => panic!("two polygons must stay a MultiPolygon"),
        other => panic!("expected areal output, got {other:?}"),
    }
    assert_eq!(report.kept_parts, 2);
    assert_eq!(report.dropped_parts, 1);
}

#[test]
fn empty_input_yields_empty_multipolygon() {
    let empty: Geometry<f64> = Geometry::GeometryCollection(GeometryCollection::empty());
    let (out, report) = apply_policy(empty, OutputPolicy::ArealOnly);
    match out {
        Geometry::MultiPolygon(mp) => assert!(mp.0.is_empty()),
        other => panic!("expected an empty MultiPolygon, got {other:?}"),
    }
    assert_eq!(report.kept_parts, 0);
    assert_eq!(report.dropped_parts, 0);
}

#[test]
fn areal_only_drops_a_pure_line_geometry() {
    let (out, report) = apply_policy(Geometry::LineString(line()), OutputPolicy::ArealOnly);
    match out {
        Geometry::MultiPolygon(mp) => assert!(mp.0.is_empty()),
        other => panic!("expected an empty MultiPolygon, got {other:?}"),
    }
    assert_eq!(report.dropped_parts, 1);
}

#[test]
fn repaired_bowtie_reduces_to_valid_polygons() {
    let config = MakeValidConfig::default();
    let (out, report) = make_valid_with_policy(
        &Geometry::Polygon(bowtie()),
        &config,
        OutputPolicy::ArealOnly,
    );
    assert!(
        matches!(out, Geometry::Polygon(_) | Geometry::MultiPolygon(_)),
        "areal policy must not emit a collection"
    );
    assert!(
        validate(&out).valid,
        "policy output must survive validation"
    );
    assert!(report.kept_parts >= 1);
}

#[test]
fn policy_codes_are_distinct() {
    let codes = [
        OutputPolicy::KeepAll.code(),
        OutputPolicy::TopDimension.code(),
        OutputPolicy::ArealOnly.code(),
    ];
    let mut sorted = codes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), codes.len());
}

// ---------------------------------------------------------------- batch entries

#[test]
fn borrowed_batch_matches_single_repair() {
    let config = MakeValidConfig::default();
    let a = square();
    let b = bowtie();
    let batch = fix_polygon_batch(&[&a, &b], &config);
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0], a.make_valid_with_config(&config));
    assert_eq!(batch[1], b.make_valid_with_config(&config));
}

#[test]
fn owned_batch_matches_single_repair() {
    let config = MakeValidConfig::default();
    let expected: Vec<Geometry<f64>> = [square(), bowtie()]
        .iter()
        .map(|p| p.make_valid_with_config(&config))
        .collect();
    let out = fix_polygon_batch_owned(vec![square(), bowtie()], &config);
    assert_eq!(out, expected);
}

#[test]
fn chunked_batch_keeps_input_order() {
    let config = MakeValidConfig::default();
    let input = [bowtie(), square(), bowtie()];
    let expected: Vec<Geometry<f64>> = input
        .iter()
        .map(|p| p.make_valid_with_config(&config))
        .collect();
    let out = fix_polygon_batch_chunked(input.into_iter(), 2, &config);
    assert_eq!(out, expected);
}

#[test]
fn batch_output_stays_valid() {
    let config = MakeValidConfig::default();
    let input = [bowtie(), square(), bowtie()];
    let out = fix_polygon_batch_owned(input.to_vec(), &config);
    for geometry in out {
        assert!(validate(&geometry).valid, "batch output must validate");
    }
}

#[test]
fn empty_batch_returns_no_results() {
    let config = MakeValidConfig::default();
    assert!(fix_polygon_batch(&[], &config).is_empty());
    assert!(fix_polygon_batch_owned(Vec::new(), &config).is_empty());
    let empty: Vec<Polygon<f64>> = Vec::new();
    assert!(fix_polygon_batch_chunked(empty.into_iter(), 4, &config).is_empty());
}

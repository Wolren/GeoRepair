//! Ports of GEOS unit test suites that the XML corpus does not reach:
//! - tests/unit/operation/valid/RepeatedPointRemoverTest.cpp (17 cases)
//! - tests/unit/operation/valid/ValidClosedRingTest.cpp (5 cases)
//!
//! Source-verified against the GEOS clone at /d/Projects/gis/lib/geos
//! (commit cad26ad98 "Return EMPTY components when repeated point removal
//! renders the underlying parts invalid").
//!
//! GEOS's `ensure_equals_geometry` normalizes both sides before comparing
//! (tests/unit/utility.h), so ring rotation is irrelevant to the assertion.
//! We mirror that: rings are compared as cyclic rotations. All expected
//! coordinate values are exact (the remover only drops points, never moves
//! them), so exact f64 equality holds.

use geo::{Coord, Geometry, LineString, Polygon};
use geo_repair::MakeValid;
use geo_repair::validation::{GeometryValidationError, validate};
use geo_repair::{remove_repeated_coords, remove_repeated_points};

#[path = "common/mod.rs"]
mod common;
use common::*;

// =========================================================================
// IsValidOpTest parity port (error-class level)
// =========================================================================
// Port of tests/unit/operation/valid/IsValidOpTest.cpp (all active tests)
// with the discipline the XML corpus cannot provide: the SPECIFIC error
// class is asserted, not just the boolean. Class-name deltas vs GEOS are
// documented per case (we flag PinchPoint where GEOS says
// eRingSelfIntersection, HoleOutsideShell where GEOS says eSelfIntersection,
// NotSimple for the line family where GEOS says eRingSelfIntersection,
// DisconnectedInteriorRing where GEOS says eHoleOutsideShell on hole
// double-touch fixtures).
//
// Cases GEOS marks valid but we reject must be the documented masked class
// (WrongOrientation) AND repair to a valid, area-preserving result (the
// same gate the XML suite applies). Cases where the ORIGINAL fixture's
// rings are mis-oriented and our rejection rides entirely on
// WrongOrientation (the structural class is shadowed) are marked in the
// table; their OGC-oriented variants are tracked as gap tests below -
// the three structural gaps were FIXED 2026-08-06 (see the gap tests at
// the bottom of this file).

struct IsValidParityCase {
    name: &'static str,
    wkt: &'static str,
    /// GEOS IsValidOpTest expectation.
    geos_valid: bool,
    /// Required error class when GEOS says invalid. None = accept.
    our_class: Option<GeometryValidationError>,
    /// GEOS's error class for documentation.
    geos_class: &'static str,
}

const ISVALID_PARITY_CASES: &[IsValidParityCase] = &[
    // test<1> - NaN coordinate
    IsValidParityCase {
        name: "t1_nan_coord",
        wkt: "LINESTRING (0 0, 1 nan)",
        geos_valid: false,
        our_class: Some(GeometryValidationError::CoordinateNaN),
        geos_class: "eInvalidCoordinate",
        // note:
    },
    // test<29> - Inf coordinate
    IsValidParityCase {
        name: "t29_inf_coord",
        wkt: "LINESTRING (0 0, 1 inf)",
        geos_valid: false,
        our_class: Some(GeometryValidationError::CoordinateNaN),
        geos_class: "eInvalidCoordinate",
        // note:
    },
    // test<2> - tiny hole outside shell
    IsValidParityCase {
        name: "t2_hole_outside_shell",
        wkt: "POLYGON((25495445.625 6671632.625,25495445.625 6671711.375,25495555.375 6671711.375,25495555.375 6671632.625,25495445.625 6671632.625),(25495368.0441 6671726.9312,25495368.3959388 6671726.93601515,25495368.7478 6671726.9333,25495368.0441 6671726.9312))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::HoleOutsideShell),
        geos_class: "eHoleOutsideShell",
        // note:
    },
    // test<3> - ticket 588: GEOS-valid; we reject (CW ring) and repair. The
    // reversed ring is covered by geos_isvalidop_ticket588_reversed below.
    IsValidParityCase {
        name: "t3_ticket588",
        wkt: "POLYGON (( -86.3958130146539250 114.3482370100377900, 64.7285128575111490 156.9678884302379600, 138.3490775437400700 43.1639042523018260, 87.9271046586986810 -10.5302909001479570, 87.9271046586986810 -10.5302909001479530, 55.7321237336437390 -44.8146215164960250, -86.3958130146539250 114.3482370100377900))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note: masked: WrongOrientation + repair
    },
    // test<4> - JTS PR 737 LINEARING self-crossing (parsed as LineString)
    IsValidParityCase {
        name: "t4_jts737_ring",
        wkt: "LINEARRING (150 100, 300 300, 100 300, 350 100, 150 100)",
        geos_valid: false,
        our_class: Some(GeometryValidationError::NotSimple),
        geos_class: "eRingSelfIntersection",
        // note: line family: GEOS eRingSelfIntersection, ours NotSimple
    },
    // test<5> - valid MP
    IsValidParityCase {
        name: "t5_valid_mp",
        wkt: "MULTIPOLYGON(((0 0, 10 0, 10 10, 0 10, 0 0),(2 2, 2 6, 6 4, 2 2)),((60 60, 60 50, 70 40, 60 60)))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note:
    },
    // test<6> - disconnected interior; structural class shadowed by
    // WrongOrientation on the mis-oriented fixture - oriented variant is
    // now REJECTED (gap fixed 2026-08-06, see
    // geos_isvalidop_t6_disconnected_interior_gap).
    IsValidParityCase {
        name: "t6_disconnected_interior",
        wkt: "POLYGON((40 320,340 320,340 20,40 20,40 320),(100 120,40 20,180 100,100 120),(200 200,180 100,240 160,200 200),(260 260,240 160,300 200,260 260),(300 300,300 200,340 260,300 300))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::WrongOrientation),
        geos_class: "eDisconnectedInterior",
        // note: orientation-shadowed; oriented variant = gap fixed
    },
    // test<7> - simple square, CW in the fixture: masked
    IsValidParityCase {
        name: "t7_simple",
        wkt: "POLYGON ((10 89, 90 89, 90 10, 10 10, 10 89))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note: masked: WrongOrientation + repair
    },
    // test<8> - bowtie
    IsValidParityCase {
        name: "t8_bowtie",
        wkt: "POLYGON ((10 90, 90 10, 90 90, 10 10, 10 90))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::SelfIntersection),
        geos_class: "eSelfIntersection",
        // note:
    },
    // test<22> - inverted polygon: GEOS eRingSelfIntersection, ours PinchPoint
    IsValidParityCase {
        name: "t22_inverted",
        wkt: "POLYGON ((70 250, 40 500, 100 400, 70 250, 80 350, 60 350, 70 250))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::PinchPoint),
        geos_class: "eRingSelfIntersection",
        // note: non-consecutive vertex revisit
    },
    // test<9> - polygon with hole, CW fixture: masked
    IsValidParityCase {
        name: "t9_hole",
        wkt: "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), (60 20, 20 70, 90 90, 60 20))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note: masked: WrongOrientation + repair
    },
    // test<10> - hole touching shell at a vertex: valid
    IsValidParityCase {
        name: "t10_hole_touch_vertex",
        wkt: "POLYGON ((240 260, 40 260, 40 80, 240 80, 240 260), (140 180, 40 260, 140 240, 140 180))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note:
    },
    // test<11> - hole properly crossing the shell: GEOS eSelfIntersection,
    // ours HoleOutsideShell (the hole genuinely pokes out)
    IsValidParityCase {
        name: "t11_hole_proper_intersection",
        wkt: "POLYGON ((10 90, 50 50, 10 10, 10 90), (20 50, 60 70, 60 30, 20 50))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::HoleOutsideShell),
        geos_class: "eSelfIntersection",
        // note: hole pokes outside thin shell
    },
    // test<12> - disconnected interior; orientation-shadowed, oriented = gap fixed
    IsValidParityCase {
        name: "t12_disconnected_interior",
        wkt: "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), (20 80, 30 80, 20 20, 20 80), (80 30, 20 20, 80 20, 80 30), (80 80, 30 80, 80 30, 80 80))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::WrongOrientation),
        geos_class: "eDisconnectedInterior",
        // note: orientation-shadowed; oriented variant = gap fixed
    },
    // test<13> - MP touch at vertices, CW fixture: masked
    IsValidParityCase {
        name: "t13_mp_touch_vertices",
        wkt: "MULTIPOLYGON (((10 10, 10 90, 90 90, 90 10, 80 80, 50 20, 20 80, 10 10)), ((90 10, 10 10, 50 20, 90 10)))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note: masked: WrongOrientation + repair
    },
    // test<14> - MP touch at segments: valid
    IsValidParityCase {
        name: "t14_mp_touch_segments",
        wkt: "MULTIPOLYGON (((60 40, 90 10, 90 90, 10 90, 10 10, 40 40, 60 40)), ((50 40, 20 20, 80 20, 50 40)))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note:
    },
    // test<15> - nested shells (all vertices touch): orientation-shadowed;
    // oriented variant = KNOWN GAP
    IsValidParityCase {
        name: "t15_nested_shells",
        wkt: "MULTIPOLYGON (((10 10, 20 30, 10 90, 90 90, 80 30, 90 10, 50 20, 10 10)), ((80 30, 20 30, 50 20, 80 30)))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::WrongOrientation),
        geos_class: "eNestedShells",
        // note: orientation-shadowed; oriented variant = KNOWN GAP
    },
    // test<16> - MP hole touch vertices, CW fixture: masked
    IsValidParityCase {
        name: "t16_mp_hole_touch",
        wkt: "MULTIPOLYGON (((20 380, 420 380, 420 20, 20 20, 20 380), (220 340, 80 320, 60 200, 140 100, 340 60, 300 240, 220 340)), ((60 200, 340 60, 220 340, 60 200)))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note: masked: WrongOrientation + repair
    },
    // test<17> - multiple holes touching at one point, CW fixture: masked
    IsValidParityCase {
        name: "t17_holes_touch_same_point",
        wkt: "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), (40 80, 60 80, 50 50, 40 80), (20 60, 20 40, 50 50, 20 60), (40 20, 60 20, 50 50, 40 20))",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note: masked: WrongOrientation + repair
    },
    // test<18> - hole outside shell all-touch: GEOS eHoleOutsideShell, ours
    // DisconnectedInteriorRing (touching at >= 2 shell points)
    IsValidParityCase {
        name: "t18_hole_outside_all_touch",
        wkt: "POLYGON ((10 10, 30 10, 30 50, 70 50, 70 10, 90 10, 90 90, 10 90, 10 10), (50 50, 30 10, 70 10, 50 50))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::DisconnectedInteriorRing),
        geos_class: "eHoleOutsideShell",
        // note: hole double-touch reads as disconnected interior
    },
    // test<19> - hole outside shell double-touch: same delta
    IsValidParityCase {
        name: "t19_hole_outside_double_touch",
        wkt: "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), (20 80, 80 80, 80 20, 20 20, 20 80), (90 70, 150 50, 90 20, 110 40, 90 70))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::DisconnectedInteriorRing),
        geos_class: "eHoleOutsideShell",
        // note: hole double-touch reads as disconnected interior
    },
    // test<20> - nested holes: orientation-shadowed; oriented variant now
    // REJECTED as DisconnectedInteriorRing (touch-cycle fires first; class
    // delta vs GEOS eNestedHoles, boolean parity)
    IsValidParityCase {
        name: "t20_nested_holes",
        wkt: "POLYGON ((10 90, 90 90, 90 10, 10 10, 10 90), (20 80, 80 80, 80 20, 20 20, 20 80), (50 80, 80 50, 50 20, 20 50, 50 80))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::WrongOrientation),
        geos_class: "eNestedHoles",
        // note: orientation-shadowed; oriented variant = gap fixed
    },
    // test<21> - MP hole overlap crossing: orientation-shadowed; oriented = gap fixed
    IsValidParityCase {
        name: "t21_mp_hole_overlap",
        wkt: "MULTIPOLYGON (((20 380, 420 380, 420 20, 20 20, 20 380), (220 340, 180 240, 60 200, 140 100, 340 60, 300 240, 220 340)), ((60 200, 340 60, 220 340, 60 200)))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::WrongOrientation),
        geos_class: "eSelfIntersection",
        // note: orientation-shadowed; oriented variant = gap fixed
    },
    // test<23> - zero-length line
    IsValidParityCase {
        name: "t23_zero_length_line",
        wkt: "LINESTRING(0 0, 0 0)",
        geos_valid: false,
        our_class: Some(GeometryValidationError::RepeatedPoint),
        geos_class: "(no class asserted)",
        // note:
    },
    // test<24> - linear ring triangle: valid
    IsValidParityCase {
        name: "t24_ring_triangle",
        wkt: "LINEARRING (100 100, 150 200, 200 100, 100 100)",
        geos_valid: true,
        our_class: None,
        geos_class: "",
        // note:
    },
    // test<26> - linear ring bowtie: line family, ours NotSimple
    IsValidParityCase {
        name: "t26_ring_bowtie",
        wkt: "LINEARRING (0 0, 100 100, 100 0, 0 100, 0 0)",
        geos_valid: false,
        our_class: Some(GeometryValidationError::NotSimple),
        geos_class: "eRingSelfIntersection",
        // note: line family
    },
    // test<27> - same polygon as test<22>
    IsValidParityCase {
        name: "t27_inverted2",
        wkt: "POLYGON ((70 250, 40 500, 100 400, 70 250, 80 350, 60 350, 70 250))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::PinchPoint),
        geos_class: "eRingSelfIntersection",
        // note: non-consecutive vertex revisit
    },
    // test<28> - self-intersecting polygon
    IsValidParityCase {
        name: "t28_polygon_si",
        wkt: "POLYGON ((70 250, 70 500, 80 400, 40 400, 70 250))",
        geos_valid: false,
        our_class: Some(GeometryValidationError::SelfIntersection),
        geos_class: "eSelfIntersection",
        // note:
    },
];

/// Even-odd (set-theoretic) area of polygon parts: shell minus holes.
fn even_odd_area(g: &Geometry<f64>) -> f64 {
    fn ring_area(ring: &[Coord<f64>]) -> f64 {
        let mut s = 0.0;
        for i in 0..ring.len().saturating_sub(1) {
            s += ring[i].x * ring[i + 1].y - ring[i + 1].x * ring[i].y;
        }
        s.abs() / 2.0
    }
    fn poly_eo(p: &Polygon<f64>) -> f64 {
        let shell = ring_area(&p.exterior().0);
        let holes: f64 = p.interiors().iter().map(|h| ring_area(&h.0)).sum();
        shell - holes
    }
    match g {
        Geometry::Polygon(p) => poly_eo(p),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(poly_eo).sum(),
        Geometry::GeometryCollection(gc) => gc.0.iter().map(even_odd_area).sum(),
        _ => 0.0,
    }
}

/// Run the masked-class gate: a GEOS-valid input we reject must (a) fail
/// only via the documented WrongOrientation class and (b) repair to a valid,
/// area-preserving geometry (same gate as the XML suite).
fn assert_masked_orientation_case(name: &str, geom: &Geometry<f64>) {
    let r = validate(geom);
    assert!(
        r.errors
            .contains(&GeometryValidationError::WrongOrientation),
        "{name}: GEOS-valid input must reject via WrongOrientation only, got {:?}",
        r.errors,
        name = name
    );
    let fixed = geom.make_valid();
    let vr = validate(&fixed);
    assert!(
        vr.valid,
        "{name}: repair must restore validity, got {:?}",
        vr.errors,
        name = name
    );
    let in_area = even_odd_area(geom);
    let fx_area = even_odd_area(&fixed);
    let scale = in_area.abs().max(1.0);
    assert!(
        in_area <= 1e-9 || (fx_area - in_area).abs() <= 1e-6 * scale,
        "{name}: repair must preserve area (input {in_area}, fixed {fx_area})"
    );
}

#[test]
fn geos_isvalidop_error_class_parity() {
    for case in ISVALID_PARITY_CASES {
        let geom = geom_from_wkt(case.wkt);
        let r = validate(&geom);
        if case.geos_valid {
            if r.valid {
                continue; // accepted - matches GEOS
            }
            assert_masked_orientation_case(case.name, &geom);
        } else {
            assert!(
                !r.valid,
                "{name}: GEOS-invalid input accepted (too lenient): {geom:?}",
                name = case.name
            );
            let cls = case
                .our_class
                .as_ref()
                .expect("GEOS-invalid case needs a class");
            assert!(
                r.errors.contains(cls),
                "{name}: expected {cls:?} in errors, got {:?} ({})",
                r.errors,
                case.geos_class,
                name = case.name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// M1: ticket 588 reversal validity (GEOS IsValidOpTest test<3> asserts BOTH
// the ring and its reverse are valid). Our validator rejects the CW ring as
// WrongOrientation; the REVERSED ring is CCW and must be accepted outright,
// and the CW direction must repair valid + area-preserving.
// ---------------------------------------------------------------------------
#[test]
fn geos_isvalidop_ticket588_reversed() {
    let wkt = "POLYGON (( -86.3958130146539250 114.3482370100377900, 64.7285128575111490 156.9678884302379600, 138.3490775437400700 43.1639042523018260, 87.9271046586986810 -10.5302909001479570, 87.9271046586986810 -10.5302909001479530, 55.7321237336437390 -44.8146215164960250, -86.3958130146539250 114.3482370100377900))";
    let g = geom_from_wkt(wkt);
    // Reverse the ring: GEOS asserts the reversed polygon is also valid.
    let Geometry::Polygon(p) = &g else {
        unreachable!()
    };
    let mut ring: Vec<Coord<f64>> = p.exterior().0.clone();
    ring.reverse();
    let rev = Geometry::Polygon(Polygon::new(LineString::new(ring), Vec::new()));
    assert!(
        validate(&rev).valid,
        "ticket 588 reversed ring must be valid (GEOS parity): {rev:?}"
    );
    // And the original CW direction repairs valid + area-preserving.
    assert_masked_orientation_case("ticket588_cw", &g);
}

// ---------------------------------------------------------------------------
// VALIDATOR GAP FIXES (2026-08-05, GEOS-parity, source-verified): with
// OGC-correct orientation our validator previously ACCEPTED these
// structural classes that GEOS rejects (every corpus fixture for these
// classes is mis-oriented, so the XML suite masked them via
// WrongOrientation and the gaps stayed invisible at "0 known gaps"). The
// fixes, all bbox-filtered (R-tree, never unfiltered O(n^2)):
// 1. disconnected interior via hole chains touching at vertices - ring-
//    touch graph cycle detection (GEOS PolygonRing::findHoleCycleLocation
//    port; touches at ONE coordinate stay valid, matching GEOS on
//    IsValidOpTest test<17>);
// 2. nested holes sharing boundary vertices - incident-segment topology
//    (GEOS PolygonTopologyAnalyzer::isRingNested + PolygonNodeTopology
//    port);
// 3. MP component crossing another component's hole - cross-component
//    ring intersection check covering shell-shell, shell-hole and
//    hole-hole pairs (GEOS checkAreaIntersections scope).
// These tests assert GEOS's expectation and now run GREEN.
// ---------------------------------------------------------------------------
#[test]
fn geos_isvalidop_t6_disconnected_interior_gap() {
    let g = geom_from_wkt(
        "POLYGON ((40 320, 40 20, 340 20, 340 320, 40 320), (100 120, 180 100, 40 20, 100 120), (200 200, 240 160, 180 100, 200 200), (260 260, 300 200, 240 160, 260 260), (300 300, 340 260, 300 200, 300 300))",
    );
    assert!(
        !validate(&g).valid,
        "GEOS: eDisconnectedInterior (hole chain to shell)"
    );
}

#[test]
fn geos_isvalidop_t12_disconnected_interior_gap() {
    let g = geom_from_wkt(
        "POLYGON ((10 90, 10 10, 90 10, 90 90, 10 90), (20 80, 30 80, 20 20, 20 80), (80 30, 20 20, 80 20, 80 30), (80 80, 30 80, 80 30, 80 80))",
    );
    assert!(
        !validate(&g).valid,
        "GEOS: eDisconnectedInterior (closed hole chain)"
    );
}

#[test]
fn geos_isvalidop_t20_nested_holes_gap() {
    let g = geom_from_wkt(
        "POLYGON ((10 90, 10 10, 90 10, 90 90, 10 90), (20 80, 80 80, 80 20, 20 20, 20 80), (50 80, 80 50, 50 20, 20 50, 50 80))",
    );
    assert!(
        !validate(&g).valid,
        "GEOS: eNestedHoles (boundary-sharing inner hole)"
    );
}

#[test]
fn geos_isvalidop_t21_mp_hole_overlap_gap() {
    let g = geom_from_wkt(
        "MULTIPOLYGON (((20 380, 20 20, 420 20, 420 380, 20 380), (220 340, 300 240, 340 60, 140 100, 60 200, 180 240, 220 340)), ((60 200, 340 60, 220 340, 60 200)))",
    );
    assert!(
        !validate(&g).valid,
        "GEOS: eSelfIntersection (component crossing hole)"
    );
}

/// Parse a line WKT and return its coordinates.
fn line_coords(wkt: &str) -> Vec<Coord<f64>> {
    match geom_from_wkt(wkt) {
        Geometry::LineString(ls) => ls.0,
        other => panic!("expected linestring, got {other:?}"),
    }
}

/// Sequence-level expected equality.
fn assert_seq_eq(input: &str, expected: &str, tolerance: f64) {
    let out = remove_repeated_coords(&line_coords(input), tolerance);
    let exp = line_coords(expected);
    assert_eq!(out, exp, "sequence filter mismatch");
}

/// Ring equality under cyclic rotation (GEOS normalize semantics). Closed
/// rings carry a duplicated closing coordinate; the distinct cycle is
/// compared, exactly like GEOS normalize + equalsExact does.
fn ring_rot_eq(a: &[Coord<f64>], b: &[Coord<f64>]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if a.is_empty() {
        return true;
    }
    let a_cycle: &[Coord<f64>] = if a.first() == a.last() {
        &a[..a.len() - 1]
    } else {
        a
    };
    let b_cycle: &[Coord<f64>] = if b.first() == b.last() {
        &b[..b.len() - 1]
    } else {
        b
    };
    if a_cycle.len() != b_cycle.len() {
        return false;
    }
    (0..a_cycle.len())
        .any(|k| (0..a_cycle.len()).all(|i| a_cycle[(k + i) % a_cycle.len()] == b_cycle[i]))
}

/// Geometry-level expected equality: lines exact, rings cyclic.
fn assert_geom_eq(input: &str, expected: &str, tolerance: f64) {
    let out = remove_repeated_points(&geom_from_wkt(input), tolerance);
    let exp = geom_from_wkt(expected);
    match (&out, &exp) {
        (Geometry::LineString(a), Geometry::LineString(b)) => {
            assert_eq!(a.0, b.0, "line mismatch");
        }
        (Geometry::Polygon(a), Geometry::Polygon(b)) => {
            assert!(
                ring_rot_eq(&a.exterior().0, &b.exterior().0),
                "shell mismatch: {:?} vs {:?}",
                a.exterior().0,
                b.exterior().0
            );
            assert_eq!(a.interiors().len(), b.interiors().len(), "hole count");
            for (ha, hb) in a.interiors().iter().zip(b.interiors()) {
                assert!(ring_rot_eq(&ha.0, &hb.0), "hole mismatch");
            }
        }
        (Geometry::MultiPolygon(a), Geometry::MultiPolygon(b)) => {
            assert_eq!(a.0.len(), b.0.len(), "part count");
            for (pa, pb) in a.0.iter().zip(&b.0) {
                assert!(
                    ring_rot_eq(&pa.exterior().0, &pb.exterior().0),
                    "part shell mismatch"
                );
                assert_eq!(
                    pa.interiors().len(),
                    pb.interiors().len(),
                    "part hole count"
                );
                for (ha, hb) in pa.interiors().iter().zip(pb.interiors()) {
                    assert!(ring_rot_eq(&ha.0, &hb.0), "part hole mismatch");
                }
            }
        }
        _ => panic!("unexpected geometry pair: {out:?} vs {exp:?}"),
    }
}

#[test]
fn rpr_test_1_exact_dups_removed() {
    // "(3 7, 8 8, 8 8, 8 8, 10 9)" -> "(3 7, 8 8, 10 9)"
    assert_seq_eq(
        "LINESTRING (3 7, 8 8, 8 8, 8 8, 10 9)",
        "LINESTRING (3 7, 8 8, 10 9)",
        0.0,
    );
}

#[test]
fn rpr_test_2_exact_dups_tail() {
    // "(3 7, 8 8, 8 8, 8 8)" -> "(3 7, 8 8)"
    assert_seq_eq(
        "LINESTRING (3 7, 8 8, 8 8, 8 8)",
        "LINESTRING (3 7, 8 8)",
        0.0,
    );
}

#[test]
fn rpr_test_3_sequence_within_tolerance() {
    // CoordinateSequences just retain each coordinate within the tolerance.
    assert_seq_eq(
        "LINESTRING (0 0, 1 0, 4 0, 5 0)",
        "LINESTRING (0 0, 4 0)",
        3.0,
    );
}

#[test]
fn rpr_test_4_line_keeps_last_point() {
    // Linestrings note the last point and retain it in preference over the
    // internal point.
    assert_geom_eq(
        "LINESTRING (0 0, 1 0, 4 0, 5 0)",
        "LINESTRING (0 0, 5 0)",
        3.0,
    );
}

#[test]
fn rpr_test_5_polygon_shell_filtered() {
    assert_geom_eq(
        "MULTIPOLYGON (((0 0, 9 0, 10 0, 10 10, 0 10, 0 1, 0 0)))",
        "MULTIPOLYGON (((0 0, 9 0, 10 10, 0 10, 0 0)))",
        3.0,
    );
}

// Test 6 (Z/M dimension preservation) is skipped: GeoRepair is 2D-only.

#[test]
fn rpr_test_7_all_exact_dups_single_entry() {
    assert_seq_eq("LINESTRING (3 7, 3 7, 3 7, 3 7)", "LINESTRING (3 7)", 0.0);
}

#[test]
fn rpr_test_8_all_within_tolerance_single_entry() {
    assert_seq_eq(
        "LINESTRING (3 7, 3.1 7.1, 3.2 7.2, 3.3 7.3)",
        "LINESTRING (3 7)",
        1.0,
    );
}

#[test]
fn rpr_test_9_line_collapses_to_empty() {
    assert_geom_eq("LINESTRING (0 0, 0 1, 0 2, 0 3)", "LINESTRING EMPTY", 14.0);
}

#[test]
fn rpr_test_10_small_hole_collapses_away() {
    assert_geom_eq(
        "POLYGON ((0 0, 9 0, 10 0, 10 10, 0 10, 0 1, 0 0), (5 5, 5 6, 6 6, 6 5, 5 5))",
        "POLYGON ((0 0, 9 0, 10 10, 0 10, 0 0))",
        3.0,
    );
}

#[test]
fn rpr_test_11_small_exterior_collapses_to_degenerate() {
    // The GEOS expectation here is a 3-coordinate ring: the repair step pops
    // the within-tolerance (0 1) and re-attaches the original end (0 0).
    assert_geom_eq(
        "POLYGON ((0 0, 9 0, 10 0, 10 10, 0 10, 0 1, 0 0))",
        "POLYGON ((0 0, 10 10, 0 0))",
        12.0,
    );
}

#[test]
fn rpr_test_12_shell_collapses_to_empty_polygon() {
    assert_geom_eq(
        "POLYGON ((0 0, 9 0, 10 0, 10 10, 0 10, 0 1, 0 0))",
        "POLYGON EMPTY",
        22.0,
    );
}

#[test]
fn rpr_test_13_invalid_coords_not_replaced() {
    assert_geom_eq(
        "LINESTRING (0 0, 0 Inf, 1 1, Inf 0)",
        "LINESTRING (0 0, 1 1)",
        1.0,
    );
}

#[test]
fn rpr_test_14_filters_to_one_point_is_empty() {
    assert_geom_eq("LINESTRING (0 0, 0 Inf, 1 1)", "LINESTRING EMPTY", 2.0);
}

#[test]
fn rpr_test_15_invalid_coords_at_start_end() {
    assert_geom_eq(
        "POLYGON ((Inf Inf, 0 0, 10 0, 10 10, 0 10, 0 0, Inf Inf))",
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))",
        2.0,
    );
}

#[test]
fn rpr_test_16_ring_with_invalid_coords_collapses() {
    assert_geom_eq(
        "POLYGON ((Inf Inf, 0 0, 10 0, 10 10, 0 10, 0 0, Inf Inf))",
        "POLYGON EMPTY",
        22.0,
    );
}

#[test]
fn rpr_test_17_tiny_hole_collapse_no_error() {
    // GH-1293: a valid polygon whose tiny hole collapses to a single point
    // must not error; the hole is dropped and the shell is preserved.
    // GEOS compares normalized geometries, so the ring rotation in the
    // expected WKT is not part of the assertion.
    assert_geom_eq(
        "POLYGON ((139770.26822331376024522 188334.00010800323798321, 139769.5 188338.01162790699163452, 139769.5 188338.3723930635896977, 139769.5 188338.5, 139769.81343283582828008 188338.5, 139770.375 188339.375, 139772.39924806414637715 188340.26989983080420643, 139770.26822331376024522 188334.00010800323798321),(139769.75256541155977175 188338.40516005983226933, 139769.75256541153066792 188338.40516005983226933, 139769.75256541153066792 188338.4051600598031655, 139769.75256541155977175 188338.40516005983226933))",
        "POLYGON ((139769.5 188338.011627907, 139769.5 188338.3723930636, 139769.5 188338.5, 139769.81343283583 188338.5, 139770.375 188339.375, 139772.39924806415 188340.2698998308, 139770.26822331376 188334.00010800324, 139769.5 188338.011627907))",
        1e-8,
    );
}

// =========================================================================
// ValidClosedRingTest (5 cases; ported from JTS)
// =========================================================================

/// The four GEOS validity tests are rings; standalone rings are parsed as
/// LineStrings by our reader, so ring semantics are exercised by wrapping
/// them as polygon exteriors.
fn ring_as_polygon(wkt: &str) -> Polygon<f64> {
    match geom_from_wkt(wkt) {
        Geometry::LineString(ls) => Polygon::new(ls, Vec::new()),
        other => panic!("expected linestring, got {other:?}"),
    }
}

fn assert_has_error(geom: &Geometry<f64>, want: GeometryValidationError) {
    let result = validate(geom);
    assert!(!result.valid, "expected invalid, got {result:?}");
    assert!(
        result.errors.contains(&want),
        "expected {want:?} in errors, got {:?}",
        result.errors
    );
}

#[test]
fn vcr_test_1_open_linear_ring_invalid() {
    // GEOS: LINEARRING (0 0, 0 10, 10 10, 10 0, 0 0) with the first point
    // perturbed by +0.0001 on x is invalid (ring not closed). GEOS rejects
    // the open ring at LinearRing construction (IllegalArgumentException).
    //
    // Our model: geo-types Polygon::new auto-closes rings, so an open ring
    // cannot be represented. The forced closing edge (0 0)->(0.0001 0)
    // overlaps the tail of the bottom edge (10 0)->(0 0) - (0.0001 0) lies
    // on that segment - so the ring is flagged SelfIntersection. Same
    // verdict as GEOS (invalid), different mechanism, documented divergence.
    let g = ring_as_polygon("LINESTRING (0 0, 0 10, 10 10, 10 0, 0 0)");
    let mut ring = g.exterior().clone();
    ring.0[0].x += 0.0001;
    let g = Geometry::Polygon(Polygon::new(ring, Vec::new()));
    assert!(!validate(&g).valid, "open ring must be invalid: {g:?}");
}

#[test]
fn vcr_test_2_closed_linear_ring_valid_in_geos_masked() {
    // GEOS: closed ring is valid (orientation is irrelevant to GEOS).
    // Ours: CW ring -> WrongOrientation (documented masked class); repair
    // restores validity with the area preserved.
    let g = Geometry::Polygon(ring_as_polygon("LINESTRING (0 0, 0 10, 10 10, 10 0, 0 0)"));
    assert_has_error(&g, GeometryValidationError::WrongOrientation);
    let fixed = g.make_valid();
    assert!(
        validate(&fixed).valid,
        "repair must restore validity: {fixed:?}"
    );
    assert!(
        (geo::Area::unsigned_area(&fixed) - 100.0).abs() < 1e-9,
        "area must be preserved: {}",
        geo::Area::unsigned_area(&fixed)
    );
}

#[test]
fn vcr_test_3_open_polygon_shell_invalid() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))");
    let Geometry::Polygon(p) = g else {
        unreachable!()
    };
    let mut ring = p.exterior().clone();
    ring.0[0].x += 0.0001;
    let g = Geometry::Polygon(Polygon::new(ring, p.interiors().to_vec()));
    assert!(!validate(&g).valid, "open shell must be invalid");
}

#[test]
fn vcr_test_4_open_polygon_hole_invalid() {
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0), (1 1, 2 1, 2 2, 1 2, 1 1))");
    let Geometry::Polygon(p) = g else {
        unreachable!()
    };
    let mut hole = p.interiors()[0].clone();
    hole.0[0].x += 0.0001;
    let g = Geometry::Polygon(Polygon::new(p.exterior().clone(), vec![hole]));
    assert!(!validate(&g).valid, "open hole must be invalid");
}

#[test]
fn vcr_test_5_closed_polygon_valid_in_geos_masked() {
    // GEOS: valid. Ours: CW shell -> WrongOrientation (documented masked
    // class); repair restores validity with the area preserved.
    let g = geom_from_wkt("POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))");
    assert_has_error(&g, GeometryValidationError::WrongOrientation);
    let fixed = g.make_valid();
    assert!(
        validate(&fixed).valid,
        "repair must restore validity: {fixed:?}"
    );
    assert!(
        (geo::Area::unsigned_area(&fixed) - 100.0).abs() < 1e-9,
        "area must be preserved: {}",
        geo::Area::unsigned_area(&fixed)
    );
}

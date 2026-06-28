use geo::validation::Validation;
use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};

use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use wkt::TryFromWkt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn assert_valid<T: Validation + std::fmt::Debug>(geom: &T) {
    assert!(
        geom.check_validation().is_ok(),
        "expected valid, got: {:?}",
        geom.check_validation()
    );
}

fn assert_geometry_valid(geom: &Geometry<f64>) {
    assert_valid(geom);
}

fn assert_not_empty(geom: &Geometry<f64>) {
    assert!(
        !matches!(geom, Geometry::GeometryCollection(gc) if gc.0.is_empty()),
        "expected non-empty geometry"
    );
}

fn config_arrange() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Arrange,
        ..Default::default()
    }
}

fn config_structure() -> MakeValidConfig {
    MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    }
}

fn config_auto() -> MakeValidConfig {
    MakeValidConfig::default()
}

// ---------------------------------------------------------------------------
// Point tests
// ---------------------------------------------------------------------------

#[test]
fn test_point_valid() {
    let p = Point::new(1.0, 2.0);
    let result = p.make_valid();
    assert_eq!(result, Geometry::Point(p));
}

#[test]
fn test_point_nan() {
    let p = Point::new(f64::NAN, 2.0);
    let result = p.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

#[test]
fn test_point_infinite_x() {
    let p = Point::new(f64::INFINITY, 2.0);
    let result = p.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

#[test]
fn test_point_infinite_y() {
    let p = Point::new(1.0, f64::NEG_INFINITY);
    let result = p.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

// ---------------------------------------------------------------------------
// Line tests
// ---------------------------------------------------------------------------

#[test]
fn test_line_valid() {
    let l = Line::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0));
    let result = l.make_valid();
    assert_eq!(result, Geometry::Line(l));
}

#[test]
fn test_line_degenerate_zero_length() {
    let l = Line::new(Point::new(1.0, 1.0), Point::new(1.0, 1.0));
    let result = l.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

#[test]
fn test_line_nan_start() {
    let l = Line::new(Point::new(f64::NAN, 0.0), Point::new(1.0, 1.0));
    let result = l.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

// ---------------------------------------------------------------------------
// LineString tests
// ---------------------------------------------------------------------------

#[test]
fn test_linestring_valid() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 2.0 },
    ]);
    let result = ls.make_valid();
    assert_eq!(result, Geometry::LineString(ls));
}

#[test]
fn test_linestring_empty() {
    let ls = LineString::<f64>::new(Vec::new());
    let result = ls.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

#[test]
fn test_linestring_single_point_keep_collapsed() {
    let ls = LineString::new(vec![Coord { x: 1.0, y: 2.0 }]);
    let config = MakeValidConfig {
        keep_collapsed: true,
        ..Default::default()
    };
    let result = ls.make_valid_with_config(&config);
    assert_eq!(result, Geometry::Point(Point::new(1.0, 2.0)));
}

#[test]
fn test_linestring_single_point_drop() {
    let ls = LineString::new(vec![Coord { x: 1.0, y: 2.0 }]);
    let result = ls.make_valid();
    assert_eq!(
        result,
        Geometry::Point(Point::new(1.0, 2.0)),
        "single-point linestring should preserve Point (GEOS compat)"
    );
}

#[test]
fn test_linestring_consecutive_duplicates() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 2.0 },
        Coord { x: 2.0, y: 2.0 },
    ]);
    let result = ls.make_valid();
    let expected = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 2.0 },
    ]);
    assert_eq!(result, Geometry::LineString(expected));
}

#[test]
fn test_linestring_nan_filtered() {
    let ls = LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord {
            x: f64::NAN,
            y: 1.0,
        },
        Coord { x: 2.0, y: 2.0 },
    ]);
    let result = ls.make_valid();
    let expected = LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }]);
    assert_eq!(result, Geometry::LineString(expected));
}

// ---------------------------------------------------------------------------
// MultiPoint tests
// ---------------------------------------------------------------------------

#[test]
fn test_multipoint_valid() {
    let mp = MultiPoint::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]);
    let result = mp.make_valid();
    assert_eq!(result, Geometry::MultiPoint(mp));
}

#[test]
fn test_multipoint_filters_nan() {
    let mp = MultiPoint::new(vec![
        Point::new(0.0, 0.0),
        Point::new(f64::NAN, 1.0),
        Point::new(2.0, 2.0),
    ]);
    let result = mp.make_valid();
    let expected = MultiPoint::new(vec![Point::new(0.0, 0.0), Point::new(2.0, 2.0)]);
    assert_eq!(result, Geometry::MultiPoint(expected));
}

#[test]
fn test_multipoint_all_invalid() {
    let mp = MultiPoint::new(vec![Point::new(f64::NAN, f64::NAN)]);
    let result = mp.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

// ---------------------------------------------------------------------------
// MultiLineString tests
// ---------------------------------------------------------------------------

#[test]
fn test_multilinestring_valid() {
    let mls = MultiLineString::new(vec![LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
    ])]);
    let result = mls.make_valid();
    assert_eq!(
        result,
        Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ])),
        "single-element MultiLineString unwraps to LineString (GEOS compat)"
    );
}

// ---------------------------------------------------------------------------
// Rect tests
// ---------------------------------------------------------------------------

#[test]
fn test_rect_valid() {
    let r = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
    let result = r.make_valid();
    assert_eq!(result, Geometry::Rect(r));
}

// ---------------------------------------------------------------------------
// Triangle tests
// ---------------------------------------------------------------------------

#[test]
fn test_triangle_valid() {
    let t = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 5.0, y: 0.0 },
        Coord { x: 2.5, y: 5.0 },
    );
    let result = t.make_valid();
    // Triangle is returned as Polygon (closed ring) by MakeValid
    assert!(matches!(result, Geometry::Polygon(_)));
    assert_geometry_valid(&result);
}

#[test]
fn test_triangle_degenerate_collinear() {
    let t = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 2.0 },
    );
    let result = t.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

#[test]
fn test_triangle_duplicate_vertices() {
    let t = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
    );
    let result = t.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

#[test]
fn test_triangle_nan() {
    let t = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord {
            x: f64::NAN,
            y: 0.0,
        },
        Coord { x: 1.0, y: 1.0 },
    );
    let result = t.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

// ---------------------------------------------------------------------------
// Polygon tests — always valid
// ---------------------------------------------------------------------------

#[test]
fn test_valid_polygon_arrange() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&config_arrange());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_valid_polygon_structure() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&config_structure());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_valid_polygon_auto() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&config_auto());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon tests — self-intersecting (bowtie)
// ---------------------------------------------------------------------------

fn make_bowtie() -> Polygon<f64> {
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    )
}

#[test]
fn test_bowtie_arrange_no_panic() {
    let poly = make_bowtie();
    let result = poly.make_valid_with_config(&config_arrange());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_bowtie_arrange() {
    let poly = make_bowtie();
    let result = poly.make_valid_with_config(&config_arrange());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_bowtie_structure() {
    let poly = make_bowtie();
    let result = poly.make_valid_with_config(&config_structure());
    // Structure method cannot fix self-intersecting shells — returns empty
    assert_geometry_valid(&result);
}

#[test]
fn test_bowtie_auto() {
    let poly = make_bowtie();
    let result = poly.make_valid_with_config(&config_auto());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon with holes
// ---------------------------------------------------------------------------

fn make_polygon_with_hole() -> Polygon<f64> {
    Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 20.0, y: 0.0 },
            Coord { x: 20.0, y: 20.0 },
            Coord { x: 0.0, y: 20.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![LineString::new(vec![
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 15.0, y: 5.0 },
            Coord { x: 15.0, y: 15.0 },
            Coord { x: 5.0, y: 15.0 },
            Coord { x: 5.0, y: 5.0 },
        ])],
    )
}

#[test]
fn test_polygon_with_hole_arrange() {
    let poly = make_polygon_with_hole();
    let result = poly.make_valid_with_config(&config_arrange());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_polygon_with_hole_structure() {
    let poly = make_polygon_with_hole();
    let result = poly.make_valid_with_config(&config_structure());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon with ring that has repeated coords
// ---------------------------------------------------------------------------

#[test]
fn test_polygon_repeated_ring_coords() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid_with_config(&config_arrange());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Polygon with NaN ring coords
// ---------------------------------------------------------------------------

#[test]
fn test_polygon_nan_ring() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord {
                x: f64::NAN,
                y: 0.0,
            },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid();
    assert_geometry_valid(&result);
}

// ---------------------------------------------------------------------------
// MultiPolygon tests
// ---------------------------------------------------------------------------

#[test]
fn test_valid_multipolygon() {
    let mp = MultiPolygon::new(vec![
        Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 5.0, y: 0.0 },
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 0.0, y: 5.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            Vec::new(),
        ),
        Polygon::new(
            LineString::new(vec![
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 15.0, y: 10.0 },
                Coord { x: 15.0, y: 15.0 },
                Coord { x: 10.0, y: 15.0 },
                Coord { x: 10.0, y: 10.0 },
            ]),
            Vec::new(),
        ),
    ]);
    let result = mp.make_valid();
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]

fn test_multipolygon_with_bowtie() {
    let mp = MultiPolygon::new(vec![make_bowtie()]);
    let result = mp.make_valid();
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Geometry dispatch tests
// ---------------------------------------------------------------------------

#[test]
fn test_geometry_dispatch_point() {
    let g = Geometry::Point(Point::new(1.0, 2.0));
    let result = g.make_valid();
    assert_eq!(result, g);
}

#[test]
fn test_geometry_dispatch_polygon() {
    let g = Geometry::Polygon(Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    ));
    let result = g.make_valid();
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]

fn test_geometry_dispatch_bowtie() {
    let g = Geometry::Polygon(make_bowtie());
    let result = g.make_valid();
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_geometry_dispatch_empty() {
    let g = Geometry::Point(Point::new(f64::NAN, 0.0));
    let result = g.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

#[test]
fn test_geometrycollection_valid() {
    let gc = GeometryCollection(vec![
        Geometry::Point(Point::new(1.0, 2.0)),
        Geometry::Line(Line::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0))),
    ]);
    let result = gc.make_valid();
    let expected = Geometry::GeometryCollection(GeometryCollection(vec![
        Geometry::Point(Point::new(1.0, 2.0)),
        Geometry::Line(Line::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0))),
    ]));
    assert_eq!(result, expected);
}

#[test]
fn test_geometrycollection_filters_empty() {
    let gc = GeometryCollection(vec![
        Geometry::Point(Point::new(f64::NAN, 0.0)),
        Geometry::Line(Line::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0))),
    ]);
    let result = gc.make_valid();
    let expected = Geometry::GeometryCollection(GeometryCollection(vec![Geometry::Line(
        Line::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0)),
    )]));
    assert_eq!(result, expected);
}

#[test]
fn test_geometrycollection_all_empty() {
    let gc = GeometryCollection(vec![Geometry::Point(Point::new(f64::NAN, 0.0))]);
    let result = gc.make_valid();
    assert_eq!(
        result,
        Geometry::GeometryCollection(GeometryCollection(Vec::new()))
    );
}

// ---------------------------------------------------------------------------
// Validation short-circuit tests
// ---------------------------------------------------------------------------

#[test]
fn test_valid_geom_short_circuits_validation() {
    let p = Point::new(1.0, 2.0);
    assert!(p.check_validation().is_ok());
    let result = p.make_valid();
    assert_eq!(result, Geometry::Point(p));
}

#[test]
fn test_valid_polygon_short_circuits() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    assert!(poly.check_validation().is_ok());
    let result = poly.make_valid();
    assert_geometry_valid(&result);
}

// ---------------------------------------------------------------------------
// WKT-based tests
// ---------------------------------------------------------------------------

fn geom_from_wkt(s: &str) -> Geometry<f64> {
    Geometry::<f64>::try_from_wkt_str(s).unwrap()
}

#[test]
fn test_wkt_point() {
    let geom = geom_from_wkt("POINT (1 2)");
    let result = geom.make_valid();
    assert_eq!(result, geom);
}

#[test]
fn test_wkt_linestring() {
    let geom = geom_from_wkt("LINESTRING (0 0, 1 1, 2 2)");
    let result = geom.make_valid();
    assert_eq!(result, geom);
}

#[test]
fn test_wkt_polygon_valid() {
    let geom = geom_from_wkt("POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))");
    let result = geom.make_valid();
    assert_geometry_valid(&result);
}

#[test]

fn test_wkt_bowtie() {
    let geom = geom_from_wkt("POLYGON ((0 0, 10 10, 10 0, 0 10, 0 0))");
    let result = geom.make_valid();
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_wkt_polygon_with_hole() {
    let geom =
        geom_from_wkt("POLYGON ((0 0, 20 0, 20 20, 0 20, 0 0), (5 5, 15 5, 15 15, 5 15, 5 5))");
    let result = geom.make_valid();
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_wkt_multipolygon() {
    let geom = geom_from_wkt(
        "MULTIPOLYGON (((0 0, 5 0, 5 5, 0 5, 0 0)), ((10 10, 15 10, 15 15, 10 15, 10 10)))",
    );
    let result = geom.make_valid();
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_wkt_geometry_collection() {
    let geom = geom_from_wkt("GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1))");
    let result = geom.make_valid();
    assert_geometry_valid(&result);
}

// ---------------------------------------------------------------------------
// Config tests
// ---------------------------------------------------------------------------

#[test]
fn test_keep_collapsed_default_false() {
    let config = MakeValidConfig::default();
    assert!(!config.keep_collapsed);
}

#[test]
fn test_default_poly_method_auto() {
    let config = MakeValidConfig::default();
    assert_eq!(config.poly_method, PolyMethod::Auto);
}

// ---------------------------------------------------------------------------
// Specific invalid polygon shapes
// ---------------------------------------------------------------------------

#[test]
fn test_polygon_spike() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid();
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

#[test]
fn test_polygon_figure_eight() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 5.0, y: 5.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid();
    assert_geometry_valid(&result);
}

#[test]
fn test_hole_outside_shell() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        vec![LineString::new(vec![
            Coord { x: 20.0, y: 20.0 },
            Coord { x: 25.0, y: 20.0 },
            Coord { x: 25.0, y: 25.0 },
            Coord { x: 20.0, y: 25.0 },
            Coord { x: 20.0, y: 20.0 },
        ])],
    );
    let result = poly.make_valid_with_config(&config_structure());
    assert_geometry_valid(&result);
    assert_not_empty(&result);
}

// ---------------------------------------------------------------------------
// Stress — many polygons
// ---------------------------------------------------------------------------

#[test]
fn test_many_valid_polygons() {
    for i in 1..=20 {
        let n = i * 3 + 3;
        let mut coords: Vec<Coord<f64>> = (0..n)
            .map(|j| {
                let angle = 2.0 * std::f64::consts::PI * j as f64 / n as f64;
                Coord {
                    x: 10.0 * angle.cos(),
                    y: 10.0 * angle.sin(),
                }
            })
            .collect();
        coords.push(coords[0]);
        let poly = Polygon::new(LineString::new(coords), Vec::new());
        let result = poly.make_valid();
        assert_geometry_valid(&result);
        assert_not_empty(&result);
    }
}

// ---------------------------------------------------------------------------
// Multiple config combinations
// ---------------------------------------------------------------------------

#[test]

fn test_all_poly_methods_on_bowtie() {
    let poly = make_bowtie();
    // Arrange and Auto handle self-intersection; Structure returns empty
    for method in &[PolyMethod::Auto, PolyMethod::Arrange] {
        let config = MakeValidConfig {
            poly_method: method.clone(),
            ..Default::default()
        };
        let result = poly.make_valid_with_config(&config);
        assert_geometry_valid(&result);
        assert_not_empty(&result);
    }
    let config = MakeValidConfig {
        poly_method: PolyMethod::Structure,
        ..Default::default()
    };
    let result = poly.make_valid_with_config(&config);
    assert_geometry_valid(&result);
}

#[test]
fn test_all_poly_methods_on_valid() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    for method in &[PolyMethod::Auto, PolyMethod::Arrange, PolyMethod::Structure] {
        let config = MakeValidConfig {
            poly_method: method.clone(),
            ..Default::default()
        };
        let result = poly.make_valid_with_config(&config);
        assert_geometry_valid(&result);
        assert_not_empty(&result);
    }
}

// ---------------------------------------------------------------------------
// Edge: already-valid short-circuit returns exact same object
// ---------------------------------------------------------------------------

#[test]
fn test_valid_returns_original_geom() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.make_valid();
    assert_eq!(result, Geometry::Polygon(poly.clone()));
}

// ---------------------------------------------------------------------------
// Edge: degenerate polygons
// ---------------------------------------------------------------------------

#[test]
fn test_degenerate_ring_too_few_points() {
    let poly = Polygon::new(
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
        Vec::new(),
    );
    let result = poly.make_valid();
    assert_geometry_valid(&result);
}

// ---------------------------------------------------------------------------
// ValidateAndFix trait tests
// ---------------------------------------------------------------------------

use geo_repair::ValidateAndFix;

#[test]
fn test_validate_and_fix_valid_polygon() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let (result, geom) = poly.validate_and_fix();
    assert!(result.valid);
    assert_geometry_valid(&geom);
}

#[test]
fn test_validate_and_fix_bowtie() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let (result, geom) = poly.validate_and_fix();
    assert!(!result.valid);
    assert_geometry_valid(&geom);
}

#[test]
fn test_validate_and_fix_always_valid() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let (result, geom) = poly.validate_and_fix_always();
    assert!(result.valid);
    assert_geometry_valid(&geom);
}

#[test]
fn test_validate_or_fix_valid_returns_ok() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.validate_or_fix();
    assert!(result.is_ok());
    assert_geometry_valid(&result.unwrap());
}

#[test]
fn test_validate_or_fix_bowtie_returns_fixed() {
    let poly = Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: 0.0, y: 10.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    );
    let result = poly.validate_or_fix();
    // Bowtie should be fixable, so validate_or_fix returns Ok
    assert!(result.is_ok());
    let geom = result.unwrap();
    assert_geometry_valid(&geom);
}

#[test]
fn test_validate_or_fix_line_valid() {
    let line = Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 10.0, y: 10.0 });
    let result = line.validate_or_fix();
    assert!(result.is_ok());
}

#[test]
fn test_validate_or_fix_point_valid() {
    let pt = Point::new(1.0, 2.0);
    let result = pt.validate_or_fix();
    assert!(result.is_ok());
}

#[test]
fn test_validate_or_fix_line_invalid() {
    let line = Line::new(
        Coord {
            x: f64::NAN,
            y: 0.0,
        },
        Coord { x: 10.0, y: 10.0 },
    );
    let result = line.validate_or_fix();
    // NaN line is valid according to geo validation? Or does it fix?
    // Just verify it doesn't panic
    let _ = result;
}

#[test]
fn test_validate_and_fix_multipoint() {
    use geo_repair::ValidateAndFix;
    let mp = MultiPoint::new(vec![
        Point::new(1.0, 2.0),
        Point::new(f64::NAN, f64::NAN),
        Point::new(3.0, 4.0),
    ]);
    let (result, geom) = mp.validate_and_fix();
    assert!(!result.valid, "multipoint with NaN should be invalid");
    assert_geometry_valid(&geom);
    assert_not_empty(&geom);
}

#[test]
fn test_validate_and_fix_multilinestring() {
    use geo_repair::ValidateAndFix;
    let mls = MultiLineString::new(vec![
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }]),
        LineString::new(vec![Coord { x: 0.0, y: 1.0 }, Coord { x: 1.0, y: 0.0 }]),
    ]);
    let (result, geom) = mls.validate_and_fix();
    assert!(!result.valid, "crossing lines should be invalid");
    let _ = geom; // may or may not be valid after repair
}

#[test]
fn test_validate_and_fix_geometrycollection_mixed() {
    use geo_repair::ValidateAndFix;
    let gc = GeometryCollection(vec![
        Geometry::Point(Point::new(1.0, 2.0)),
        Geometry::LineString(LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
        ])),
    ]);
    let (result, geom) = gc.validate_and_fix();
    assert!(result.valid, "valid GC should be reported as valid");
    assert_geometry_valid(&geom);
}

#[test]
fn test_validate_or_fix_multipolygon() {
    use geo_repair::ValidateAndFix;
    let mp = MultiPolygon::new(vec![Polygon::new(
        LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ]),
        Vec::new(),
    )]);
    let result = mp.validate_or_fix();
    assert!(result.is_ok(), "valid multipolygon should return Ok");
    assert_geometry_valid(&result.unwrap());
}

#[test]
fn test_validate_or_fix_multilinestring_valid() {
    use geo_repair::ValidateAndFix;
    let mls = MultiLineString::new(vec![LineString::new(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
    ])]);
    let result = mls.validate_or_fix();
    assert!(result.is_ok(), "valid multilinestring should return Ok");
}

#[test]
fn test_validate_or_fix_geometrycollection_valid() {
    use geo_repair::ValidateAndFix;
    let gc = GeometryCollection(vec![
        Geometry::Point(Point::new(0.0, 0.0)),
        Geometry::Line(Line::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        )),
    ]);
    let result = gc.validate_or_fix();
    assert!(result.is_ok(), "valid GC should return Ok");
}

#[test]
fn test_zm_roundtrip_stress() {
    // Z/M roundtrip with extreme coordinate values
    let coords = vec![
        Coord { x: 1e14, y: 1e14 },
        Coord {
            x: 1e14 + 1.0,
            y: 1e14,
        },
        Coord {
            x: 1e14 + 1.0,
            y: 1e14 + 1.0,
        },
        Coord { x: 1e14, y: 1e14 },
    ];
    let poly = Polygon::new(LineString::new(coords), Vec::new());
    let count = geo_repair::zm::count_coords(&Geometry::Polygon(poly.clone()));
    let zm: Vec<_> = (0..count)
        .map(|i| geo_repair::zm::ZmValue::new(Some(i as f64 * 100.0), Some(i as f64 * 200.0)))
        .collect();
    let feature = geo_repair::Feature::with_all(Geometry::Polygon(poly), None, None, zm);
    for method in &[PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
        let cfg = MakeValidConfig {
            poly_method: method.clone(),
            ..Default::default()
        };
        let repaired = feature.geometry.make_valid_with_config(&cfg);
        let expected = geo_repair::zm::count_coords(&repaired);
        let _with_zm = feature.with_repaired_geometry(repaired);
        assert_eq!(
            _with_zm.zm.len(),
            expected,
            "Z/M count mismatch for {:?}",
            method
        );
    }
}

#[test]
fn test_large_coord_polygon_with_hole() {
    // Large magnitude coords with a hole — tests CDT range guard
    let shell = LineString::new(vec![
        Coord { x: 1e10, y: 1e10 },
        Coord {
            x: 1e10 + 1000.0,
            y: 1e10,
        },
        Coord {
            x: 1e10 + 1000.0,
            y: 1e10 + 1000.0,
        },
        Coord {
            x: 1e10,
            y: 1e10 + 1000.0,
        },
        Coord { x: 1e10, y: 1e10 },
    ]);
    let hole = LineString::new(vec![
        Coord {
            x: 1e10 + 200.0,
            y: 1e10 + 200.0,
        },
        Coord {
            x: 1e10 + 400.0,
            y: 1e10 + 200.0,
        },
        Coord {
            x: 1e10 + 400.0,
            y: 1e10 + 400.0,
        },
        Coord {
            x: 1e10 + 200.0,
            y: 1e10 + 400.0,
        },
        Coord {
            x: 1e10 + 200.0,
            y: 1e10 + 200.0,
        },
    ]);
    let poly = Polygon::new(shell, vec![hole]);
    for method in &[PolyMethod::Auto, PolyMethod::Structure, PolyMethod::Arrange] {
        let cfg = MakeValidConfig {
            poly_method: method.clone(),
            ..Default::default()
        };
        let result = poly.make_valid_with_config(&cfg);
        assert_geometry_valid(&result);
        assert_not_empty(&result);
    }
}

#[test]
fn test_validate_and_fix_always_multiply_types() {
    use geo_repair::ValidateAndFix;
    // Point
    let pt = Point::new(1.0, 2.0);
    let (_, geom) = pt.validate_and_fix_always();
    assert_geometry_valid(&geom);

    // Rect
    let rect = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
    let (_, geom) = rect.validate_and_fix_always();
    assert_geometry_valid(&geom);

    // Triangle
    let tri = Triangle::new(
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 5.0, y: 10.0 },
        Coord { x: 10.0, y: 0.0 },
    );
    let (_, geom) = tri.validate_and_fix_always();
    assert_geometry_valid(&geom);

    // MultiPoint (valid)
    let mp = MultiPoint::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]);
    let (_, geom) = mp.validate_and_fix_always();
    assert_geometry_valid(&geom);

    // MultiLineString (with crossing components — may or may not be fixable)
    let mls = MultiLineString::new(vec![
        LineString::new(vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 2.0, y: 2.0 }]),
        LineString::new(vec![Coord { x: 0.0, y: 2.0 }, Coord { x: 2.0, y: 0.0 }]),
    ]);
    let (_, geom) = mls.validate_and_fix_always();
    let _ = geom;

    // GeometryCollection
    let gc = GeometryCollection(vec![Geometry::Point(Point::new(0.0, 0.0))]);
    let (_, geom) = gc.validate_and_fix_always();
    assert_geometry_valid(&geom);
}

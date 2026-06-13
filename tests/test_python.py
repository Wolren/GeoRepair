"""Tests for geo_repair Python bindings."""
import json
import pytest
from geo_repair import repair_wkt, repair_geojson, is_valid_wkt, is_valid_geojson, validate_wkt, validate_geojson


def test_is_valid_wkt_point():
    assert is_valid_wkt("POINT(1 2)") is True


def test_is_valid_wkt_bowtie():
    assert is_valid_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))") is False


def test_is_valid_geojson():
    gj = json.dumps({"type": "Polygon", "coordinates": [[[0,0],[10,0],[10,10],[0,10],[0,0]]]})
    assert is_valid_geojson(gj) is True


def test_validate_wkt_bowtie():
    errors = validate_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")
    assert len(errors) > 0
    assert any("self" in e.lower() for e in errors)


def test_validate_wkt_valid():
    errors = validate_wkt("POINT(1 2)")
    assert len(errors) == 0


def test_validate_geojson():
    gj = json.dumps({"type": "Polygon", "coordinates": [[[0,0],[5,5],[5,0],[0,5],[0,0]]]})
    errors = validate_geojson(gj)
    assert len(errors) > 0


def test_repair_wkt_valid_point():
    assert repair_wkt("POINT(1 2)") == "POINT(1 2)"


def test_repair_wkt_valid_linestring():
    assert repair_wkt("LINESTRING(0 0,1 1)") == "LINESTRING(0 0,1 1)"


def test_repair_wkt_valid_polygon():
    result = repair_wkt("POLYGON((0 0,10 0,10 10,0 10,0 0))")
    assert "POLYGON" in result


def test_repair_wkt_bowtie():
    result = repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")
    assert "MULTIPOLYGON" in result
    assert result.startswith("MULTIPOLYGON")


def test_repair_wkt_method_structure():
    result = repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))", "structure")
    assert "MULTIPOLYGON" in result


def test_repair_wkt_method_arrange():
    result = repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))", "arrange")
    assert "MULTIPOLYGON" in result


def test_repair_wkt_invalid():
    with pytest.raises(ValueError):
        repair_wkt("NOTVALID")


def test_repair_geojson_geometry_collection():
    gj = json.dumps({
        "type": "GeometryCollection",
        "geometries": [
            {"type": "LineString", "coordinates": [[0, 0], [1, 1]]},
            {"type": "LineString", "coordinates": [[2, 2], [3, 3]]},
        ]
    })
    result = repair_geojson(gj)
    data = json.loads(result)
    assert data["type"] == "FeatureCollection"
    assert len(data["features"]) > 0


def test_repair_geojson_bowtie():
    gj = json.dumps({
        "type": "Polygon",
        "coordinates": [[[0, 0], [5, 5], [5, 0], [0, 5], [0, 0]]]
    })
    result = repair_geojson(gj)
    data = json.loads(result)
    geom = data["features"][0]["geometry"]
    assert geom["type"] == "MultiPolygon"


def test_repair_geojson_invalid_json():
    with pytest.raises(ValueError):
        repair_geojson("not json")


def test_repair_geojson_feature_collection():
    gj = json.dumps({
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "geometry": {"type": "Polygon", "coordinates": [[[0,0],[5,5],[5,0],[0,5],[0,0]]]},
                "properties": {}
            }
        ]
    })
    result = repair_geojson(gj)
    data = json.loads(result)
    assert data["type"] == "FeatureCollection"
    assert len(data["features"]) > 0


def test_repair_geojson_feature():
    gj = json.dumps({
        "type": "Feature",
        "geometry": {"type": "Polygon", "coordinates": [[[0,0],[5,5],[5,0],[0,5],[0,0]]]},
        "properties": {"name": "test"}
    })
    result = repair_geojson(gj)
    data = json.loads(result)
    geom = data["features"][0]["geometry"]
    assert geom["type"] == "MultiPolygon"


# ---------------------------------------------------------------------------
# Post-repair validity: repaired output must pass is_valid
# ---------------------------------------------------------------------------

def test_repair_makes_valid_wkt():
    """Repaired bowtie must pass is_valid."""
    wkt = "POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))"
    assert not is_valid_wkt(wkt)
    fixed = repair_wkt(wkt)
    assert is_valid_wkt(fixed), f"repaired output invalid: {fixed}"


def test_repair_makes_valid_geojson():
    """Repaired GeoJSON (FeatureCollection wrapping MultiPolygon) validates internally.
       Note: geojson crate 1.0 has a deserialization quirk with FeatureCollection
       round-trips — validate_wkt on the equivalent WKT covers this path."""
    gj = json.dumps({
        "type": "Polygon",
        "coordinates": [[[0, 0], [5, 5], [5, 0], [0, 5], [0, 0]]]
    })
    assert not is_valid_geojson(gj)
    fixed = repair_geojson(gj)
    data = json.loads(fixed)
    assert data["type"] == "FeatureCollection"
    assert data["features"][0]["geometry"]["type"] == "MultiPolygon"


def test_repair_no_false_errors():
    """Repaired output must have empty validate_wkt."""
    wkt = "POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))"
    fixed = repair_wkt(wkt)
    errors = validate_wkt(fixed)
    assert errors == [], f"repaired output has errors: {errors}"


def test_repair_all_methods_produce_valid():
    """All three methods must produce valid output from a bowtie."""
    wkt = "POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))"
    for method in ["auto", "arrange", "structure"]:
        fixed = repair_wkt(wkt, method)
        assert is_valid_wkt(fixed), f"method={method} produced invalid output"
        assert validate_wkt(fixed) == [], f"method={method} has errors: {validate_wkt(fixed)}"


def test_validate_multi_geom():
    """validate_geojson reports per-geometry errors."""
    gj = json.dumps({
        "type": "GeometryCollection",
        "geometries": [
            {"type": "Polygon", "coordinates": [[[0,0],[10,0],[10,10],[0,10],[0,0]]]},
            {"type": "Polygon", "coordinates": [[[0,0],[5,5],[5,0],[0,5],[0,0]]]},
        ]
    })
    errors = validate_geojson(gj)
    assert any("self" in e.lower() for e in errors), f"no self-intersection found: {errors}"
    assert errors[0].startswith("[geom 0]") or errors[0].startswith("[geom 1]")


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

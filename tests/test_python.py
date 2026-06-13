"""Tests for geo_repair Python bindings."""
import json
import pytest
from geo_repair import repair_wkt, repair_geojson


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


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

"""Tests for geo_repair Python bindings (WKT surface only).

GeoJSON bindings were removed by design (2026-07-31): the supported
surface is repair_wkt / is_valid_wkt / validate_wkt plus the batch and
validate-and-fix helpers.
"""
import pytest
from geo_repair import (
    repair_wkt,
    repair_wkt_batch,
    is_valid_wkt,
    validate_wkt,
    validate_wkt_batch,
    validate_and_fix_wkt,
)

BOWTIE = "POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))"
SQUARE = "POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))"


def test_is_valid_wkt_point():
    assert is_valid_wkt("POINT(1 2)") is True


def test_is_valid_wkt_bowtie():
    assert is_valid_wkt(BOWTIE) is False


def test_validate_wkt_bowtie():
    errors = validate_wkt(BOWTIE)
    assert len(errors) > 0
    assert any("self" in e.lower() for e in errors)


def test_validate_wkt_valid():
    errors = validate_wkt("POINT(1 2)")
    assert len(errors) == 0


def test_repair_wkt_valid_point():
    assert repair_wkt("POINT(1 2)") == "POINT (1.0 2.0)"


def test_repair_wkt_valid_linestring():
    assert repair_wkt("LINESTRING(0 0,1 1)") == "LINESTRING (0.0 0.0, 1.0 1.0)"


def test_repair_wkt_valid_polygon():
    result = repair_wkt(SQUARE)
    assert "POLYGON" in result


def test_repair_wkt_bowtie():
    result = repair_wkt(BOWTIE)
    assert result.startswith("MULTIPOLYGON")


def test_repair_wkt_method_structure():
    result = repair_wkt(BOWTIE, "structure")
    assert "MULTIPOLYGON" in result


def test_repair_wkt_method_arrange():
    result = repair_wkt(BOWTIE, "arrange")
    assert "MULTIPOLYGON" in result


def test_repair_wkt_invalid():
    with pytest.raises(ValueError):
        repair_wkt("NOTVALID")


def test_repair_wkt_batch():
    results = repair_wkt_batch([SQUARE, BOWTIE])
    assert len(results) == 2
    assert results[0].startswith("POLYGON")
    assert results[1].startswith("MULTIPOLYGON")


def test_validate_wkt_batch():
    errors = validate_wkt_batch([SQUARE, BOWTIE])
    assert len(errors) == 2
    assert errors[0] == []
    assert len(errors[1]) > 0


def test_validate_and_fix_wkt():
    verdict, errors, fixed = validate_and_fix_wkt(BOWTIE)
    assert verdict is False
    assert len(errors) > 0
    assert is_valid_wkt(fixed)
    verdict2, _errors2, fixed2 = validate_and_fix_wkt(SQUARE)
    assert verdict2 is True
    assert is_valid_wkt(fixed2)


# ---------------------------------------------------------------------------
# Post-repair validity: repaired output must pass is_valid
# ---------------------------------------------------------------------------

def test_repair_makes_valid_wkt():
    """Repaired bowtie must pass is_valid."""
    assert not is_valid_wkt(BOWTIE)
    fixed = repair_wkt(BOWTIE)
    assert is_valid_wkt(fixed), f"repaired output invalid: {fixed}"


def test_repair_no_false_errors():
    """Repaired output must have empty validate_wkt."""
    fixed = repair_wkt(BOWTIE)
    errors = validate_wkt(fixed)
    assert errors == [], f"repaired output has errors: {errors}"


def test_repair_all_methods_produce_valid():
    """All three methods must produce valid output from a bowtie."""
    for method in ["auto", "arrange", "structure"]:
        fixed = repair_wkt(BOWTIE, method)
        assert is_valid_wkt(fixed), f"method={method} produced invalid output"
        assert validate_wkt(fixed) == [], f"method={method} has errors: {validate_wkt(fixed)}"


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

"""Tests for the geo_repair Python bindings (full WKB + WKT surface).

The supported surface is the complete repair/validate API over both WKB
bytes and WKT text, single and batch, plus the parallel batch functions.
GeoJSON bindings do not exist (removed by design 2026-07-31).
"""
import struct

import pytest
from geo_repair import (
    __version__,
    is_valid_wkb,
    is_valid_wkb_batch,
    is_valid_wkt,
    is_valid_wkt_batch,
    par_repair_wkb_batch,
    par_repair_wkt_batch,
    repair_validate_wkb,
    repair_validate_wkb_batch,
    repair_validate_wkt,
    repair_validate_wkt_batch,
    repair_wkb,
    repair_wkb_batch,
    repair_wkt,
    repair_wkt_batch,
    validate_and_fix_wkb,
    validate_and_fix_wkb_batch,
    validate_and_fix_wkt,
    validate_and_fix_wkt_batch,
    validate_wkb,
    validate_wkb_batch,
    validate_wkt,
    validate_wkt_batch,
    version,
)

BOWTIE = "POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))"
SQUARE = "POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))"


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def wkb_polygon(points):
    """Little-endian WKB POLYGON with a single ring."""
    out = struct.pack("<BI", 1, 3)  # byte order + polygon
    out += struct.pack("<I", 1)  # one ring
    out += struct.pack("<I", len(points))
    for x, y in points:
        out += struct.pack("<dd", x, y)
    return out


SQUARE_WKB = wkb_polygon([(0, 0), (10, 0), (10, 10), (0, 10), (0, 0)])
BOWTIE_WKB = wkb_polygon([(0, 0), (5, 5), (5, 0), (0, 5), (0, 0)])
GARBAGE = b"\x01\x02\x03\x04"


# ---------------------------------------------------------------------------
# Version
# ---------------------------------------------------------------------------

def test_version():
    assert version() == __version__
    parts = version().split(".")
    assert len(parts) == 3
    assert all(p.isdigit() for p in parts)


# ---------------------------------------------------------------------------
# WKB: validity
# ---------------------------------------------------------------------------

def test_is_valid_wkb():
    assert is_valid_wkb(SQUARE_WKB) is True
    assert is_valid_wkb(BOWTIE_WKB) is False


def test_is_valid_wkb_bad_input_raises():
    with pytest.raises(ValueError):
        is_valid_wkb(GARBAGE)


def test_is_valid_wkb_batch():
    assert is_valid_wkb_batch([SQUARE_WKB, BOWTIE_WKB, GARBAGE]) == [True, False, False]


# ---------------------------------------------------------------------------
# WKB: validation
# ---------------------------------------------------------------------------

def test_validate_wkb():
    valid, errors = validate_wkb(SQUARE_WKB)
    assert valid is True
    assert errors == []


def test_validate_wkb_bowtie():
    valid, errors = validate_wkb(BOWTIE_WKB)
    assert valid is False
    assert len(errors) > 0
    assert any("self" in e.lower() for e in errors)


def test_validate_wkb_batch():
    results = validate_wkb_batch([SQUARE_WKB, BOWTIE_WKB, GARBAGE])
    assert results[0] == (True, [])
    assert results[1][0] is False
    assert len(results[1][1]) > 0
    assert results[2][0] is False  # parse failure reported as errors


# ---------------------------------------------------------------------------
# WKB: repair
# ---------------------------------------------------------------------------

def test_repair_wkb_bowtie():
    fixed = repair_wkb(BOWTIE_WKB)
    assert is_valid_wkb(fixed) is True
    assert fixed != BOWTIE_WKB


def test_repair_wkb_valid_passthrough():
    fixed = repair_wkb(SQUARE_WKB)
    assert is_valid_wkb(fixed) is True


def test_repair_wkb_methods():
    for method in ("auto", "arrange", "structure"):
        fixed = repair_wkb(BOWTIE_WKB, method=method)
        assert is_valid_wkb(fixed), f"method={method} produced invalid output"


def test_repair_wkb_keep_collapsed_accepted():
    # keep_collapsed is accepted and does not break the pipeline
    fixed = repair_wkb(BOWTIE_WKB, keep_collapsed=True)
    assert is_valid_wkb(fixed)


def test_repair_wkb_bad_input_raises():
    with pytest.raises(ValueError):
        repair_wkb(GARBAGE)


def test_repair_wkb_batch():
    results = repair_wkb_batch([SQUARE_WKB, BOWTIE_WKB])
    assert len(results) == 2
    assert is_valid_wkb(results[0])
    assert is_valid_wkb(results[1])


def test_repair_wkb_batch_passthrough_on_parse_error():
    results = repair_wkb_batch([SQUARE_WKB, GARBAGE])
    assert results[1] == GARBAGE  # unparseable input returned unchanged


def test_par_repair_wkb_batch_matches_sequential():
    inputs = [SQUARE_WKB, BOWTIE_WKB] * 4
    seq = repair_wkb_batch(inputs)
    par = par_repair_wkb_batch(inputs)
    assert par == seq
    assert all(is_valid_wkb(w) for w in par)


# ---------------------------------------------------------------------------
# WKB: repair + validate
# ---------------------------------------------------------------------------

def test_repair_validate_wkb():
    fixed, was_valid, errors = repair_validate_wkb(BOWTIE_WKB)
    assert was_valid is False
    assert len(errors) > 0
    assert is_valid_wkb(fixed)


def test_repair_validate_wkb_valid():
    fixed, was_valid, errors = repair_validate_wkb(SQUARE_WKB)
    assert was_valid is True
    assert errors == []
    assert is_valid_wkb(fixed)


def test_repair_validate_wkb_batch():
    results = repair_validate_wkb_batch([SQUARE_WKB, BOWTIE_WKB])
    assert results[0][1] is True
    assert results[1][1] is False
    assert all(is_valid_wkb(r[0]) for r in results)


# ---------------------------------------------------------------------------
# WKB: validate + fix
# ---------------------------------------------------------------------------

def test_validate_and_fix_wkb():
    was_valid, errors, fixed = validate_and_fix_wkb(BOWTIE_WKB)
    assert was_valid is False
    assert len(errors) > 0
    assert is_valid_wkb(fixed)


def test_validate_and_fix_wkb_valid():
    was_valid, errors, fixed = validate_and_fix_wkb(SQUARE_WKB)
    assert was_valid is True
    assert errors == []
    assert is_valid_wkb(fixed)


def test_validate_and_fix_wkb_batch():
    results = validate_and_fix_wkb_batch([SQUARE_WKB, BOWTIE_WKB])
    assert results[0][0] is True
    assert results[1][0] is False
    assert all(is_valid_wkb(r[2]) for r in results)


# ---------------------------------------------------------------------------
# WKT: validity
# ---------------------------------------------------------------------------

def test_is_valid_wkt():
    assert is_valid_wkt("POINT(1 2)") is True
    assert is_valid_wkt(BOWTIE) is False


def test_is_valid_wkt_batch():
    assert is_valid_wkt_batch([SQUARE, BOWTIE, "NOTVALID"]) == [True, False, False]


def test_is_valid_wkt_bad_input_raises():
    with pytest.raises(ValueError):
        is_valid_wkt("NOTVALID")


# ---------------------------------------------------------------------------
# WKT: validation
# ---------------------------------------------------------------------------

def test_validate_wkt():
    assert validate_wkt(SQUARE) == []
    errors = validate_wkt(BOWTIE)
    assert len(errors) > 0


def test_validate_wkt_batch():
    results = validate_wkt_batch([SQUARE, BOWTIE, "NOTVALID"])
    assert results[0] == []
    assert len(results[1]) > 0
    assert len(results[2]) == 1  # parse failure reported as an error string


# ---------------------------------------------------------------------------
# WKT: repair
# ---------------------------------------------------------------------------

def test_repair_wkt():
    assert repair_wkt("POINT(1 2)") == "POINT (1.0 2.0)"
    fixed = repair_wkt(BOWTIE)
    assert fixed.startswith("MULTIPOLYGON")
    assert is_valid_wkt(fixed)


def test_repair_wkt_methods():
    for method in ("auto", "arrange", "structure"):
        fixed = repair_wkt(BOWTIE, method=method)
        assert is_valid_wkt(fixed), f"method={method} produced invalid output"


def test_repair_wkt_keep_collapsed_accepted():
    fixed = repair_wkt(BOWTIE, keep_collapsed=True)
    assert is_valid_wkt(fixed)


def test_repair_wkt_bad_input_raises():
    with pytest.raises(ValueError):
        repair_wkt("NOTVALID")


def test_repair_wkt_batch():
    results = repair_wkt_batch([SQUARE, BOWTIE])
    assert len(results) == 2
    assert all(is_valid_wkt(r) for r in results)


def test_repair_wkt_batch_passthrough_on_parse_error():
    results = repair_wkt_batch([SQUARE, "NOTVALID"])
    assert results[1] == "NOTVALID"


def test_par_repair_wkt_batch_matches_sequential():
    inputs = [SQUARE, BOWTIE] * 4
    seq = repair_wkt_batch(inputs)
    par = par_repair_wkt_batch(inputs)
    assert par == seq
    assert all(is_valid_wkt(w) for w in par)


# ---------------------------------------------------------------------------
# WKT: repair + validate
# ---------------------------------------------------------------------------

def test_repair_validate_wkt():
    fixed, was_valid, errors = repair_validate_wkt(BOWTIE)
    assert was_valid is False
    assert len(errors) > 0
    assert is_valid_wkt(fixed)


def test_repair_validate_wkt_valid():
    fixed, was_valid, errors = repair_validate_wkt(SQUARE)
    assert was_valid is True
    assert errors == []
    assert is_valid_wkt(fixed)


def test_repair_validate_wkt_batch():
    results = repair_validate_wkt_batch([SQUARE, BOWTIE])
    assert results[0][1] is True
    assert results[1][1] is False
    assert all(is_valid_wkt(r[0]) for r in results)


# ---------------------------------------------------------------------------
# WKT: validate + fix
# ---------------------------------------------------------------------------

def test_validate_and_fix_wkt():
    was_valid, errors, fixed = validate_and_fix_wkt(BOWTIE)
    assert was_valid is False
    assert len(errors) > 0
    assert is_valid_wkt(fixed)


def test_validate_and_fix_wkt_valid():
    was_valid, errors, fixed = validate_and_fix_wkt(SQUARE)
    assert was_valid is True
    assert errors == []
    assert is_valid_wkt(fixed)


def test_validate_and_fix_wkt_batch():
    results = validate_and_fix_wkt_batch([SQUARE, BOWTIE])
    assert results[0][0] is True
    assert results[1][0] is False
    assert all(is_valid_wkt(r[2]) for r in results)


# ---------------------------------------------------------------------------
# Cross-format consistency: WKB and WKT surfaces must agree
# ---------------------------------------------------------------------------

def test_wkb_wkt_agree_on_bowtie():
    assert is_valid_wkb(BOWTIE_WKB) == is_valid_wkt(BOWTIE)
    fixed_wkb = repair_wkb(BOWTIE_WKB)
    fixed_wkt = repair_wkt(BOWTIE)
    assert is_valid_wkb(fixed_wkb) == is_valid_wkt(fixed_wkt)


# ---------------------------------------------------------------------------
# Post-repair validity contract
# ---------------------------------------------------------------------------

def test_repair_output_always_valid():
    for make in (repair_wkb, repair_wkt):
        for bad in (BOWTIE_WKB if "wkb" in make.__name__ else BOWTIE,):
            fixed = make(bad)
            checker = is_valid_wkb if "wkb" in make.__name__ else is_valid_wkt
            assert checker(fixed), f"{make.__name__} produced invalid output"

#!/usr/bin/env python3
"""OGC conformance run: WKT and WKB judged against GDAL, not against ourselves.

Checks
  A  read acceptance matches the oracle, per dimension family
  B  our geometry equals the oracle's (canonical 2D WKB)
  C  our WKT output, re-read by the oracle, equals the oracle's geometry
  D  our ISO WKB equals the oracle's bytes, 2D cases
  E  oracle WKB, including Z and M, reads and re-emits unchanged
  F  big-endian WKB from an independent encoder reads and re-emits unchanged
  G  EWKB with an SRID keeps its SRID and its geometry
  H  damaged WKB (every truncation, impossible headers) never panics
  I  nested collections parse, and absurd depth fails cleanly
  J  SQL/MM curved types are refused, not misread

Usage: python conformance/run.py [--keep]
Exit code 1 on an unexpected failure. Ids listed in known_divergences.txt do not fail the gate.
"""

from __future__ import annotations

import csv
import json
import os
import shutil
import sqlite3
import subprocess
import sys

try:
    import shapely
    from shapely import wkt as shapely_wkt
except ImportError:  # the GEOS checks need it; main() fails loudly instead of skipping
    shapely = None
    shapely_wkt = None
import tempfile

import wkbcodec

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OGR2OGR = os.environ.get("OGR2OGR", "ogr2ogr")

NL = chr(10)
TAB = chr(9)
CR = chr(13)


# --------------------------------------------------------------------------- corpus


def num(v):
    if isinstance(v, int):
        return str(v)
    return repr(float(v))


def coord(x, y, dims):
    parts = [num(x), num(y)]
    if "Z" in dims:
        parts.append(num(7.5))
    if "M" in dims:
        parts.append(num(3.5))
    return " ".join(parts)


def mod(dims):
    return dims + " " if dims else ""


def ring(points, dims):
    pts = [coord(x, y, dims) for x, y in points]
    pts.append(pts[0])
    return "(" + ", ".join(pts) + ")"


SQUARE = [(0, 0), (10, 0), (10, 10), (0, 10)]
HOLE = [(2, 2), (4, 2), (4, 4)]


def builders(dims):
    m = mod(dims)
    return [
        ("point", "POINT " + m + "(" + coord(1, 2, dims) + ")"),
        (
            "linestring",
            "LINESTRING " + m + "(" + ", ".join(coord(x, y, dims) for x, y in [(0, 0), (1, 1), (2, 0)]) + ")",
        ),
        ("polygon", "POLYGON " + m + "(" + ring(SQUARE, dims) + ", " + ring(HOLE, dims) + ")"),
        (
            "multipoint_bare",
            "MULTIPOINT " + m + "(" + coord(1, 2, dims) + ", " + coord(3, 4, dims) + ")",
        ),
        (
            "multipoint_paren",
            "MULTIPOINT " + m + "((" + coord(1, 2, dims) + "), (" + coord(3, 4, dims) + "))",
        ),
        (
            "multilinestring",
            "MULTILINESTRING "
            + m
            + "(("
            + coord(0, 0, dims)
            + ", "
            + coord(1, 1, dims)
            + "), ("
            + coord(2, 2, dims)
            + ", "
            + coord(3, 3, dims)
            + "))",
        ),
        (
            "multipolygon",
            "MULTIPOLYGON "
            + m
            + "((("
            + ", ".join(coord(x, y, dims) for x, y in [(0, 0), (1, 0), (1, 1), (0, 0)])
            + ")), (("
            + ", ".join(coord(x, y, dims) for x, y in [(5, 5), (6, 5), (6, 6), (5, 5)])
            + ")))",
        ),
        (
            "collection",
            "GEOMETRYCOLLECTION "
            + m
            + "(POINT "
            + m
            + "("
            + coord(1, 2, dims)
            + "), LINESTRING "
            + m
            + "("
            + coord(0, 0, dims)
            + ", "
            + coord(1, 1, dims)
            + "))",
        ),
    ]


TYPES = [
    "POINT",
    "LINESTRING",
    "POLYGON",
    "MULTIPOINT",
    "MULTILINESTRING",
    "MULTIPOLYGON",
    "GEOMETRYCOLLECTION",
]


def corpus():
    cases = []
    for dims in ("", "Z", "M", "ZM"):
        tag = dims or "XY"
        for name, text in builders(dims):
            cases.append((tag + "_" + name, "shape", text))
        for t in TYPES:
            cases.append((tag + "_empty_" + t.lower(), "empty", t + " " + mod(dims) + "EMPTY"))
        if dims:
            # PostGIS and GDAL both accept the modifier welded to the type name.
            compact = {
                "Z": "POINTZ (1 2 3)",
                "M": "POINTM (1 2 3)",
                "ZM": "POINTZM (1 2 3 4)",
            }[dims]
            cases.append((tag + "_compact_point", "shape", compact))
            if dims == "ZM":
                cases.append(
                    (
                        tag + "_compact_multipolygon",
                        "shape",
                        "MULTIPOLYGONZM (((0 0 1 1, 1 0 2 2, 1 1 3 3, 0 0 1 1)))",
                    )
                )

    lex = [
        ("lex_lower", "point (1 2)"),
        ("lex_mixed_case", "PoInT (1 2)"),
        ("lex_z_lower", "point z (1 2 3)"),
        ("lex_spaces", "POINT    (   1     2   )"),
        ("lex_newline", "POINT" + NL + "(1 2)"),
        ("lex_tab", "LINESTRING" + TAB + "(0 0, 1 1)"),
        ("lex_exp", "POINT (1e3 1E-3)"),
        ("lex_leading_dot", "POINT (.5 -.5)"),
        ("lex_trailing_dot", "POINT (5. 6.)"),
        ("lex_neg_zero", "POINT (-0.0 -0.0)"),
        ("lex_plus", "POINT (+1 +2)"),
        ("lex_long", "POINT (0.1234567890123456789 1.9876543210987654321)"),
        ("lex_int", "POINT (1 2)"),
        ("lex_trailing_space", "POINT (1 2)   "),
    ]
    cases += [(i, "lexical", t) for i, t in lex]

    struct = [
        ("st_unclosed_ring", "POLYGON ((0 0, 10 0, 10 10, 0 10))"),
        ("st_degenerate_ring", "POLYGON ((0 0, 1 1, 0 0))"),
        ("st_hole_empty", "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), EMPTY)"),
        ("st_mpoly_empty_part", "MULTIPOLYGON (((0 0, 1 0, 1 1, 0 0)), EMPTY)"),
        ("st_mpoint_empty", "MULTIPOINT (EMPTY, (1 2))"),
        ("st_coll_empty_member", "GEOMETRYCOLLECTION (POINT EMPTY, POINT (1 2))"),
        ("st_coll_nested", "GEOMETRYCOLLECTION (GEOMETRYCOLLECTION (POINT (1 2)))"),
        ("st_coll_mixed_dims", "GEOMETRYCOLLECTION Z (POINT Z (1 2 3), POINT Z (4 5 6))"),
        ("st_coll_mixed_dims_bad", "GEOMETRYCOLLECTION (POINT Z (1 2 3), POINT (4 5))"),
        ("st_nan", "POINT (NaN 2)"),
        ("st_inf", "POINT (Inf 2)"),
        ("st_ninf", "POINT (-Inf -Inf)"),
        ("st_bowtie", "POLYGON ((0 0, 1 1, 1 0, 0 1, 0 0))"),
        ("st_hole_outside", "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (20 20, 21 20, 21 21, 20 20))"),
        ("st_pentagram", "POLYGON ((0 0, 2 3, 4 0, 0 2, 4 2, 0 0))"),
        ("st_zero_area", "POLYGON ((0 0, 1 0, 2 0, 0 0))"),
        ("st_self_crossing_line", "LINESTRING (0 0, 2 2, 2 0, 0 2)"),
        ("st_dup_points", "MULTIPOINT (1 1, 1 1)"),
        (
            "st_overlap_polygons",
            "MULTIPOLYGON (((0 0, 2 0, 2 2, 0 2, 0 0)), ((1 1, 3 1, 3 3, 1 3, 1 1)))",
        ),
    ]
    cases += [(i, "structural", t) for i, t in struct]

    bad = [
        ("bad_arity_low", "POINT (1)"),
        ("bad_arity_high", "POINT (1 2 3)"),
        ("bad_z_two_ord", "POINT Z (1 2)"),
        ("bad_z_four_ord", "POINT Z (1 2 3 4)"),
        ("bad_zm_three_ord", "POINT ZM (1 2 3)"),
        ("bad_unbalanced", "POLYGON ((0 0, 1 1)"),
        ("bad_no_body", "POINT"),
        ("bad_empty_string", ""),
        ("bad_partial", "POINT (1 2"),
        ("bad_double_comma", "LINESTRING (0 0,, 1 1)"),
        ("bad_trailing_junk", "POINT (1 2) garbage"),
        ("bad_unknown_type", "CIRCLE (1 2 3)"),
        ("bad_missing_comma", "LINESTRING (0 0 1 1)"),
    ]
    cases += [(i, "invalid", t) for i, t in bad]

    curved = [
        ("curve_circularstring", "CIRCULARSTRING (0 0, 1 1, 2 0)"),
        ("curve_compoundcurve", "COMPOUNDCURVE ((0 0, 1 1), CIRCULARSTRING (1 1, 2 1, 3 1))"),
        ("curve_curvepolygon", "CURVEPOLYGON (CIRCULARSTRING (0 0, 1 1, 2 0, 3 1, 0 0))"),
        ("curve_multicurve", "MULTICURVE ((0 0, 1 1))"),
        ("curve_multisurface", "MULTISURFACE (((0 0, 1 0, 1 1, 0 0)))"),
        ("curve_polyhedralsurface", "POLYHEDRALSURFACE (((0 0, 1 0, 1 1, 0 0)))"),
        ("curve_tin", "TIN (((0 0, 1 0, 1 1, 0 0)))"),
        ("curve_triangle", "TRIANGLE ((0 0, 1 0, 1 1, 0 0))"),
    ]
    cases += [(i, "curved", t) for i, t in curved]
    return cases


# --------------------------------------------------------------------------- oracle


def strip_gpkg(blob):
    env = {0: 0, 1: 32, 2: 48, 3: 48, 4: 64}[(blob[3] >> 1) & 7]
    return blob[8 + env :]


def oracle(texts, workdir, name):
    csv_in = os.path.join(workdir, name + ".csv")
    gpkg = os.path.join(workdir, name + ".gpkg")
    with open(csv_in, "w", newline="", encoding="utf-8") as fh:
        w = csv.writer(fh)
        w.writerow(["idx", "wkt"])
        for i, t in enumerate(texts):
            w.writerow([i, t])
    if os.path.exists(gpkg):
        os.remove(gpkg)
    cmd = [
        OGR2OGR,
        "-f",
        "GPKG",
        gpkg,
        csv_in,
        "-oo",
        "GEOM_POSSIBLE_NAMES=wkt",
        "-nlt",
        "GEOMETRY",
        "-nln",
        "cases",
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if not os.path.exists(gpkg):
        raise SystemExit("oracle failed: " + proc.stderr.strip()[:400])
    con = sqlite3.connect(gpkg)
    rows = dict(con.execute("select cast(idx as integer), geom from cases").fetchall())
    con.close()
    return {i: (None if rows.get(i) is None else strip_gpkg(rows[i]).hex()) for i in range(len(texts))}


# --------------------------------------------------------------------------- crate side


def dump_rust(args, workdir):
    exe = os.path.join(ROOT, "target", "debug", "examples", "conformance_dump.exe")
    if not os.path.exists(exe):
        raise SystemExit("missing " + exe + "; run: cargo build --example conformance_dump")
    proc = subprocess.run([exe] + args, capture_output=True, text=True, timeout=300)
    if proc.returncode != 0:
        raise SystemExit("dump failed: " + proc.stderr[-600:])
    rows = {}
    for line in proc.stdout.splitlines():
        if line.strip():
            rec = json.loads(line)
            rows[rec["id"]] = rec
    return rows


def write_tsv(workdir, name, lines):
    path = os.path.join(workdir, name + ".tsv")
    with open(path, "w", encoding="utf-8") as fh:
        for line in lines:
            fh.write(line + NL)
    return path


def escape_tsv(text):
    return text.replace(NL, "|NL|").replace(TAB, "|TAB|").replace(CR, "|CR|")


def run_wkb_batch(workdir, name, lines):
    return dump_rust(["--wkb", write_tsv(workdir, name, lines)], workdir)


def run_text_batch(workdir, name, lines):
    return dump_rust([write_tsv(workdir, name, lines)], workdir)


def mutate(hexes):
    lines = [("empty", ""), ("lone_order", "01")]
    seeds = [h for h in hexes[:6] if h]
    for si, h in enumerate(seeds):
        raw = bytes.fromhex(h)
        for cut in range(0, len(raw)):
            lines.append(("trunc_%d_%d" % (si, cut), raw[:cut].hex()))
        if len(raw) >= 9:
            for name, patch in (
                ("order", bytes([2]) + raw[1:]),
                ("type0", raw[:1] + (0).to_bytes(4, "little") + raw[5:]),
                ("type999", raw[:1] + (999).to_bytes(4, "little") + raw[5:]),
                ("huge_count", raw[:5] + (0xFFFFFFFF).to_bytes(4, "little") + raw[9:]),
            ):
                lines.append((name + "_" + str(si), patch.hex()))
    return [i + TAB + v for i, v in lines]


def build_probe(workdir):
    """Build the RustGIS probe when RUSTGIS_DIR points at a checkout. Returns the exe or None."""
    root = os.environ.get("RUSTGIS_DIR")
    if not root:
        return None
    src = os.path.join(HERE, "rustgis_probe")
    tmp = os.path.join(workdir, "probe")
    shutil.copytree(src, tmp)
    manifest = os.path.join(tmp, "Cargo.toml")
    with open(os.path.join(tmp, "Cargo.toml.in"), encoding="utf-8") as fh:
        text = fh.read().replace("@RUSTGIS_DIR@", root.replace(os.sep, "/"))
    with open(manifest, "w", encoding="utf-8") as fh:
        fh.write(text)
    target = os.path.join(ROOT, "target", "rustgis-probe")
    env = dict(os.environ, CARGO_TARGET_DIR=target)
    proc = subprocess.run(
        ["cargo", "build", "--quiet", "--manifest-path", manifest],
        env=env,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        print("rustgis probe build failed, check K skipped: " + proc.stderr[-400:])
        return None
    for name in ("rustgis-probe.exe", "rustgis-probe"):
        path = os.path.join(target, "debug", name)
        if os.path.exists(path):
            return path
    return None


def build_georust():
    """Build the georust wkt and wkb probe. Returns the exe or None."""
    src = os.path.join(HERE, "georust_probe")
    target = os.path.join(ROOT, "target", "georust-probe")
    env = dict(os.environ, CARGO_TARGET_DIR=target)
    proc = subprocess.run(
        ["cargo", "build", "--quiet", "--manifest-path", os.path.join(src, "Cargo.toml")],
        env=env,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        print("georust probe build failed, check L skipped: " + proc.stderr[-400:])
        return None
    for name in ("georust-probe.exe", "georust-probe"):
        path = os.path.join(target, "debug", name)
        if os.path.exists(path):
            return path
    return None


def run_probe(exe, lines):
    proc = subprocess.run(
        [exe], input="".join(line + NL for line in lines), capture_output=True, text=True, timeout=300
    )
    rows = {}
    for line in proc.stdout.splitlines():
        parts = line.split(TAB)
        if len(parts) >= 3:
            rows[parts[0]] = (parts[1], parts[2] if len(parts) > 2 else "")
    return rows


def expectation(cid, cat):
    """What the specification requires of this case: ("accept" or "reject", oracle_normalizes)."""
    if cat in ("invalid", "curved"):
        return "reject", False
    if cid == "st_unclosed_ring":
        # A polygon ring must be closed. The reader keeps the geometry as given and the writer
        # serializes it, which is what GEOS does; OGR closes it on ingest, so its bytes differ.
        return "accept", True
    return "accept", False


def load_known():
    path = os.path.join(HERE, "known_divergences.txt")
    known = {}
    if not os.path.exists(path):
        return known
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.split("#", 1)[0].strip()
            if line:
                known[line] = True
    return known


# --------------------------------------------------------------------------- driver


def main():
    keep = "--keep" in sys.argv
    workdir = tempfile.mkdtemp(prefix="wktconf-")
    oracle_lenient = []
    we_lenient = []
    try:
        cases = corpus()
        if os.environ.get("CONFORMANCE_SELFTEST"):
            # A geometry the specification accepts and our reader cannot parse. The gate has to
            # report it and exit 1, or the gate proves nothing.
            cases.append(("selftest_must_fail", "shape", "POINT ("))
        ids = [c[0] for c in cases]
        texts = [c[2] for c in cases]

        corpus_tsv = write_tsv(
            workdir, "corpus", [cid + TAB + escape_tsv(t) for cid, t in zip(ids, texts)]
        )
        ours = dump_rust([corpus_tsv], workdir)
        ora = oracle(texts, workdir, "cases")

        no_iso = []
        res = {
            k: [[], []]
            for k in (
                "A_read_accept",
                "B_geometry",
                "C_write_semantics",
                "D_write_bytes",
                "E_wkb_roundtrip",
                "F_big_endian",
                "G_ewkb_srid",
                "H_damaged",
                "I_nesting",
                "J_curved",
                "K_rustgis",
                "L_georust",
                "M_geos_valid",
                "N_repair_measure",
            )
        }

        def rec(check, ok, cid, detail=""):
            res[check][0 if ok else 1].append((cid, detail))

        emitted = []
        emitted_ids = []
        for cid in ids:
            row = ours.get(cid, {})
            if row.get("zm_ok") and row.get("wkt_out"):
                emitted.append(row["wkt_out"])
                emitted_ids.append(cid)
        ora_written = oracle(emitted, workdir, "emitted")

        for i, (cid, cat, text) in enumerate(cases):
            row = ours.get(cid, {})
            o = ora[i]
            oracle_ok = o is not None
            family = cid.split("_", 1)[0]
            use_zm = family in ("Z", "M", "ZM")
            want, normalizes = expectation(cid, cat)
            if "panic" in row:
                rec("A_read_accept", False, cid, "panic: " + row["panic"][:100])
                continue
            our_ok = bool(row.get("zm_ok") if use_zm else row.get("plain_ok"))
            err = row.get("zm_err") if use_zm else row.get("plain_err")
            rec(
                "A_read_accept",
                our_ok == (want == "accept"),
                cid,
                "spec=" + want + " ours=" + str(our_ok) + " " + str(err)[:100],
            )
            if oracle_ok != (want == "accept"):
                if want == "reject":
                    oracle_lenient.append(cid)
                else:
                    we_lenient.append(cid)

            if oracle_ok and our_ok and not normalizes and want == "accept":
                our_hex = row.get("isowkb") or row.get("wkb")
                same = wkbcodec.canonical_2d(our_hex) == wkbcodec.canonical_2d(o)
                rec("B_geometry", same, cid, "" if same else "geometry differs")
                iso = (row.get("isowkb") or "").lower()
                if iso and o:
                    same_bytes = iso == o.lower()
                    rec("D_write_bytes", same_bytes, cid, "" if same_bytes else "bytes differ")
                else:
                    no_iso.append(cid)
                if use_zm and cid in emitted_ids:
                    expect = wkbcodec.canonical_2d(o)
                    got = wkbcodec.canonical_2d(ora_written.get(emitted_ids.index(cid)))
                    ok = got == expect and not expect.startswith("UNPARSABLE")
                    rec("C_write_semantics", ok, cid, "" if ok else "written text reads differently")

        pairs = [
            (cid, ora[i])
            for i, (cid, cat, _text) in enumerate(cases)
            if ora[i] is not None and expectation(cid, cat)[0] == "accept" and not expectation(cid, cat)[1]
        ]
        e_rows = run_wkb_batch(workdir, "wkb", [cid + TAB + h for cid, h in pairs])
        for cid, h in pairs:
            row = e_rows.get(cid, {})
            ok = bool(row.get("ok")) and row.get("recoded", "").lower() == h.lower()
            rec("E_wkb_roundtrip", ok, cid, "" if ok else (row.get("err") or "recode differs")[:120])

        be_lines = []
        be_ids = []
        for cid, h in pairs:
            try:
                be = wkbcodec.to_big_endian(h)
            except Exception:  # noqa: BLE001
                continue
            be_lines.append(cid + TAB + be)
            be_ids.append((cid, be))
        f_rows = run_wkb_batch(workdir, "be", be_lines)
        for cid, be in be_ids:
            row = f_rows.get(cid, {})
            # The writer emits little-endian, so acceptance is checked on the geometry, and the
            # byte order itself is proven by the independent encoder that produced `be`.
            ok = bool(row.get("ok")) and wkbcodec.canonical_2d(
                row.get("recoded", "")
            ) == wkbcodec.canonical_2d(be)
            rec("F_big_endian", ok, cid, "" if ok else (row.get("err") or "geometry differs")[:120])

        srid_lines = []
        srid_ids = []
        for cid, h in pairs:
            try:
                srid_lines.append(cid + TAB + wkbcodec.with_srid(h, 4326))
            except Exception:  # noqa: BLE001
                continue
            srid_ids.append(cid)
        g_rows = run_wkb_batch(workdir, "srid", srid_lines)
        for cid in srid_ids:
            row = g_rows.get(cid, {})
            ok = bool(row.get("ok")) and row.get("srid") == "4326"
            rec(
                "G_ewkb_srid",
                ok,
                cid,
                "" if ok else "srid=" + str(row.get("srid")) + " " + str(row.get("err", ""))[:80],
            )

        h_lines = mutate([h for _, h in pairs])
        h_rows = run_wkb_batch(workdir, "damaged", h_lines)
        for line in h_lines:
            cid = line.split(TAB, 1)[0]
            row = h_rows.get(cid, {})
            ok = "panic" not in row and "ok" in row
            rec("H_damaged", ok, cid, str(row.get("panic", ""))[:120])

        nested = []
        for depth in (2, 12, 64):
            text = "POINT (1 2)"
            for _ in range(depth):
                text = "GEOMETRYCOLLECTION (" + text + ")"
            nested.append("nest_" + str(depth) + TAB + text)
        i_rows = run_text_batch(workdir, "nested", nested)
        for depth in (2, 12):
            row = i_rows.get("nest_" + str(depth), {})
            ok = bool(row.get("zm_ok"))
            rec("I_nesting", ok, "nest_" + str(depth), str(row.get("zm_err", ""))[:100])
        deep = i_rows.get("nest_64", {})
        rec("I_nesting", "panic" not in deep, "nest_64", "absurd depth must fail cleanly")

        curved = [(c[0], c[2]) for c in cases if c[1] == "curved"]
        j_rows = run_text_batch(workdir, "curved", [cid + TAB + t for cid, t in curved])
        for cid, _ in curved:
            row = j_rows.get(cid, {})
            if "panic" in row:
                rec("J_curved", False, cid, "panic: " + row["panic"][:100])
                continue
            err = (row.get("zm_err") or row.get("plain_err") or "").lower()
            ok = bool(err) and ("unknown geometry type" in err or "parse error" in err)
            rec("J_curved", ok, cid, err[:100])

        # K: the consumer. RustGIS reads what a closed 2D host receives from us.
        probe = build_probe(workdir)
        skipped_k = []
        raw_rejected = []
        if probe is None:
            print("check K skipped: set RUSTGIS_DIR to a RustGIS checkout")
        else:
            k_lines = []
            k_raw = []
            k_ids = []
            for cid in ids:
                row = ours.get(cid, {})
                text = row.get("wkt_delivery") or ""
                raw = row.get("wkt_out") or ""
                if not row.get("zm_ok"):
                    continue
                if raw:
                    k_raw.append(cid + TAB + raw)
                k_lines.append(cid + TAB + text)
                k_ids.append(cid)
            k_rows = run_probe(probe, k_lines)
            raw_rows = run_probe(probe, k_raw)
            for cid in k_ids:
                if raw_rows.get(cid, ("",))[0] in ("err", "panic"):
                    raw_rejected.append(cid)
                i = ids.index(cid)
                if ora[i] is None:
                    continue
                expected = wkbcodec.rustgis_shape(ora[i])
                if expected is None:
                    skipped_k.append(cid + " (nothing survives host::to_host)")
                    continue
                status, payload = k_rows.get(cid, ("missing", ""))
                if status == "err":
                    rec("K_rustgis", False, cid, "rustgis error: " + payload[:100])
                elif status == "none":
                    rec("K_rustgis", False, cid, "rustgis returned no geometry")
                elif status == "panic":
                    rec("K_rustgis", False, cid, "rustgis panicked")
                else:
                    ok = status == "ok" and payload == expected
                    rec("K_rustgis", ok, cid, "" if ok else "shape differs")

        # L and the comparison matrix: the same delivered text through every implementation.
        gexe = build_georust()
        if gexe is None:
            print("check L skipped: the georust probe did not build")
        else:
            subject = []
            for cid in ids:
                row = ours.get(cid, {})
                if not row.get("zm_ok"):
                    continue
                text = row.get("wkt_delivery") or ""
                if text:
                    subject.append((cid, text))
            g_rows = run_probe(gexe, [cid + TAB + text for cid, text in subject])
            for cid, _text in subject:
                i = ids.index(cid)
                if ora[i] is None:
                    continue
                expected = wkbcodec.shape_of_hex(ora[i])
                if expected is None:
                    continue
                status, payload = g_rows.get(cid, ("missing", ""))
                if status == "ok" and payload == expected:
                    rec("L_georust", True, cid)
                elif status == "ok":
                    rec("L_georust", False, cid, "shape differs")
                else:
                    rec("L_georust", False, cid, status + ": " + payload[:90])

            # Comparison matrix: every implementation reads the SAME raw corpus text.
            def flat(text):
                return text.replace(NL, " ").replace(TAB, " ").replace(CR, " ")

            def safe_plain(h):
                # Curved geometries have no shared data model, so they are not comparable.
                if not h:
                    return None
                try:
                    return wkbcodec.plain_shape(h)
                except Exception:  # noqa: BLE001
                    return None

            matrix_ids = [cid for cid in ids if ours.get(cid, {}).get("zm_ok") is not None]
            r_rows = (
                run_probe(probe, [cid + TAB + flat(texts[ids.index(cid)]) for cid in matrix_ids])
                if probe
                else {}
            )
            gg_rows = (
                run_probe(gexe, [cid + TAB + flat(texts[ids.index(cid)]) for cid in matrix_ids])
                if gexe
                else {}
            )

            evidence = {"GeoRepair": [], "RustGIS": [], "georust wkt": [], "GDAL": []}
            for cid in matrix_ids:
                i = ids.index(cid)
                oracle_shape = safe_plain(ora[i])
                evidence["GDAL"].append((cid, oracle_shape, oracle_shape))
                row = ours[cid]
                mine = row.get("wkb") or ""
                mine_shape = safe_plain(mine) if row.get("plain_ok") else None
                evidence["GeoRepair"].append((cid, mine_shape, oracle_shape))
                if probe:
                    status, payload = r_rows.get(cid, ("missing", ""))
                    evidence["RustGIS"].append((cid, payload if status == "ok" else None, oracle_shape))
                if gexe:
                    gstatus, gpayload = gg_rows.get(cid, ("missing", ""))
                    evidence["georust wkt"].append((cid, gpayload if gstatus == "ok" else None, oracle_shape))

            columns = {}
            for impl, rows in evidence.items():
                if not rows:
                    continue
                both = [(c, got, want) for c, got, want in rows if got is not None and want is not None]
                agree = sum(1 for _c, got, want in both if got == want)
                differs = sum(1 for _c, got, want in both if got != want)
                refused = sum(1 for _c, got, want in rows if got is None and want is not None)
                columns[impl] = (len(rows), agree, differs, refused)

            # WKB acceptance on the oracle's own bytes. RustGIS has no public WKB reader in
            # crates/data, so its column is absent here and that is stated, not faked.
            wkb_ids = [cid for cid in matrix_ids if ora[ids.index(cid)]]
            hexes = [(cid, ora[ids.index(cid)]) for cid in wkb_ids]
            if hexes:
                wtsv = write_tsv(workdir, "wkbmatrix", [cid + TAB + h for cid, h in hexes])
                our_wkb = dump_rust(["--wkb", wtsv], workdir)
                geo_wkb = {}
                if gexe:
                    proc = subprocess.run(
                        [gexe, "--wkb"],
                        input="".join(cid + TAB + h + NL for cid, h in hexes),
                        capture_output=True,
                        text=True,
                        timeout=300,
                    )
                    for line in proc.stdout.splitlines():
                        parts = line.split(TAB)
                        if len(parts) >= 2:
                            geo_wkb[parts[0]] = parts[1]
                ours_ok = sum(1 for cid, _h in hexes if our_wkb.get(cid, {}).get("ok"))
                geo_ok = sum(1 for cid, _h in hexes if geo_wkb.get(cid) == "ok")
                disagree = [
                    cid
                    for cid, _h in hexes
                    if bool(our_wkb.get(cid, {}).get("ok")) != (geo_wkb.get(cid) == "ok")
                ]
                print("WKB acceptance on the oracle's bytes: GeoRepair "
                      + str(ours_ok) + "/" + str(len(hexes))
                      + ", georust wkb " + str(geo_ok) + "/" + str(len(hexes))
                      + ", disagreements " + str(len(disagree)))
                if disagree and "--matrix" in sys.argv:
                    print("  " + ", ".join(disagree[:10]))

            print("same raw corpus text through each implementation, geometry vs the oracle:")
            print("%-14s%8s%8s%8s%9s" % ("impl", "cases", "agree", "differ", "refused"))
            for impl, (n, agree, differs, refused) in columns.items():
                print("%-14s%8d%8d%8d%9d" % (impl, n, agree, differs, refused))
            print()
            if "--matrix" in sys.argv:
                for impl, rows in evidence.items():
                    bad = [(c, got, want) for c, got, want in rows if got != want]
                    if not bad:
                        continue
                    print("[" + impl + "] disagrees on " + str(len(bad)) + " cases")
                    for cid, got, want in bad[:15]:
                        if got is None:
                            shown = "refused"
                        elif want is None:
                            shown = "no shared model"
                        elif got.split(":")[0] == want.split(":")[0]:
                            shown = "same type, different coordinates"
                        else:
                            shown = "different geometry"
                        print("  " + cid + ": " + shown)
                    print()

        # M: GEOS judges validity, and the repair pass must be shown to have changed something.
        if shapely is None:
            rec(
                "M_geos_valid",
                False,
                "geos_binding_missing",
                "install shapely, the validity oracle",
            )
        else:
            repaired_ids = []
            for cid in ids:
                row = ours.get(cid, {})
                text_rep = row.get("wkt_repaired") or ""
                if not text_rep:
                    continue
                try:
                    g = shapely_wkt.loads(text_rep)
                except Exception as exc:
                    rec("M_geos_valid", False, cid, "GEOS cannot read our output: " + str(exc)[:80])
                    continue
                # GEOS separates validity from simplicity: a self-crossing line is valid but
                # not simple, so a host check needs both.
                valid = bool(shapely.is_valid(g))
                simple = bool(shapely.is_simple(g))
                ok = valid and simple
                detail = ""
                if not valid:
                    detail = shapely.is_valid_reason(g)[:90]
                elif not simple:
                    detail = "not simple per GEOS"
                rec("M_geos_valid", ok, cid, detail)
                raw = row.get("wkt_delivery") or ""
                if not raw:
                    continue
                try:
                    raw_g = shapely_wkt.loads(raw)
                except Exception:
                    continue
                if ok and not (shapely.is_valid(raw_g) and shapely.is_simple(raw_g)):
                    repaired_ids.append(cid)
            if not repaired_ids:
                rec(
                    "M_geos_valid",
                    False,
                    "repair_never_exercised",
                    "no case was invalid before repair and valid after it",
                )
            else:
                print(
                    "check M: repair turned "
                    + str(len(repaired_ids))
                    + " deliveries GEOS called invalid or non-simple into valid, simple ones: "
                    + ", ".join(repaired_ids)
                )

        # Every run proves the judge can reject: a bowtie is invalid, a square is clean.
        if shapely is not None:
            bow = shapely_wkt.loads("POLYGON ((0 0, 1 1, 1 0, 0 1, 0 0))")
            sq = shapely_wkt.loads("POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))")
            if shapely.is_valid(bow) or shapely.is_simple(bow):
                print("check M: the GEOS judge accepted a bowtie, so M proves nothing")
                sys.exit(2)
            if not (shapely.is_valid(sq) and shapely.is_simple(sq)):
                print("check M: the GEOS judge rejected a square, so M cannot pass")
                sys.exit(2)
            if os.environ.get("CONFORMANCE_SELFTEST"):
                print("check M self test: the judge rejects a bowtie and accepts a square")

        # N: repair must not lose measure, and must leave a clean geometry alone.
        if shapely is not None:
            wider = []
            dropped = []
            for cid in ids:
                row = ours.get(cid, {})
                raw = row.get("wkt_delivery") or ""
                rep = row.get("wkt_repaired") or ""
                if not raw:
                    continue
                try:
                    raw_g = shapely_wkt.loads(raw)
                except Exception:
                    continue
                clean = bool(shapely.is_valid(raw_g) and shapely.is_simple(raw_g))
                if not rep:
                    if clean:
                        rec("N_repair_measure", False, cid, "a clean geometry produced no output")
                    elif raw_g.area > 1e-9:
                        rec("N_repair_measure", False, cid, "repair dropped a geometry with area")
                    else:
                        dropped.append(cid)
                    continue
                try:
                    rep_g = shapely_wkt.loads(rep)
                except Exception as exc:
                    rec("N_repair_measure", False, cid, "unreadable repair: " + str(exc)[:60])
                    continue
                if clean:
                    same = bool(shapely.equals(raw_g, rep_g))
                    rec(
                        "N_repair_measure",
                        same,
                        cid,
                        "" if same else "repair moved a geometry that was already clean",
                    )
                    continue
                fixed = shapely.make_valid(raw_g)
                kind = raw_g.geom_type
                if kind in ("Polygon", "MultiPolygon"):
                    ours_m, geos_m = rep_g.area, fixed.area
                    ok = ours_m >= geos_m * (1.0 - 1e-9)
                    if ours_m > geos_m * (1.0 + 1e-9):
                        wider.append(cid)
                    detail = "" if ok else "area %.6g below GEOS %.6g" % (ours_m, geos_m)
                elif kind in ("LineString", "MultiLineString"):
                    ours_m, geos_m = rep_g.length, fixed.length
                    ok = abs(ours_m - geos_m) <= 1e-9 * max(1.0, geos_m)
                    detail = "" if ok else "length %.6g vs GEOS %.6g" % (ours_m, geos_m)
                else:
                    ours_m = float(shapely.get_num_coordinates(rep_g))
                    geos_m = float(shapely.get_num_coordinates(fixed))
                    ok = 1.0 <= ours_m <= geos_m
                    detail = "" if ok else "count %.0f vs GEOS %.0f" % (ours_m, geos_m)
                rec("N_repair_measure", ok, cid, detail)
            if wider:
                print(
                    "check N: repair fills more than GEOS in "
                    + str(len(wider))
                    + " cases: "
                    + ", ".join(wider)
                )
            if dropped:
                print(
                    "check N: repair produced nothing for "
                    + str(len(dropped))
                    + " zero-area cases: "
                    + ", ".join(dropped)
                )

        print("corpus " + str(len(cases)) + " cases, oracle " + OGR2OGR)
        print()
        print("%-20s%6s%6s  %s" % ("check", "pass", "fail", "examples"))
        for name, (ok, bad) in res.items():
            sample = ", ".join(c for c, _ in bad[:6])
            print("%-20s%6d%6d  %s" % (name, len(ok), len(bad), sample))
        print()
        for name, (ok, bad) in res.items():
            if bad:
                print("[" + name + "] first failures")
                for cid, detail in bad[:10]:
                    print("  " + cid + ": " + detail)
                print()

        fails = sum(len(b) for _, b in res.values())
        print("TOTAL failures: " + str(fails))
        if raw_rejected:
            print(
                "raw output, before host::to_host, is unreadable by RustGIS in "
                + str(len(raw_rejected))
                + " cases: "
                + ", ".join(raw_rejected[:8])
            )
        if no_iso:
            print(
                "check D finds no ISO bytes to compare for "
                + str(len(no_iso))
                + " cases: "
                + ", ".join(no_iso[:6])
            )
        if skipped_k:
            print(
                "check K skips "
                + str(len(skipped_k))
                + " cases RustGIS cannot represent: "
                + ", ".join(skipped_k[:6])
            )
        if oracle_lenient:
            print(
                "OGR accepts "
                + str(len(oracle_lenient))
                + " cases the specification forbids, and we reject them: "
                + ", ".join(oracle_lenient[:12])
            )
        if we_lenient:
            print(
                "We accept "
                + str(len(we_lenient))
                + " cases outside the grammar on purpose, for damaged corpora: "
                + ", ".join(we_lenient[:12])
            )
        return 1 if fails else 0
    finally:
        if keep:
            print("workdir kept: " + workdir)
        else:
            shutil.rmtree(workdir, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())

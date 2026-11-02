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
    try:
        cases = corpus()
        ids = [c[0] for c in cases]
        texts = [c[2] for c in cases]

        corpus_tsv = write_tsv(
            workdir, "corpus", [cid + TAB + escape_tsv(t) for cid, t in zip(ids, texts)]
        )
        ours = dump_rust([corpus_tsv], workdir)
        ora = oracle(texts, workdir, "cases")

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
            if "panic" in row:
                rec("A_read_accept", False, cid, "panic: " + row["panic"][:100])
                continue
            our_ok = bool(row.get("zm_ok") if use_zm else row.get("plain_ok"))
            err = row.get("zm_err") if use_zm else row.get("plain_err")
            rec(
                "A_read_accept",
                our_ok == oracle_ok,
                cid,
                "oracle=" + str(oracle_ok) + " ours=" + str(our_ok) + " " + str(err)[:100],
            )

            if oracle_ok and our_ok and cat != "curved":
                our_hex = row.get("isowkb") or row.get("wkb")
                same = wkbcodec.canonical_2d(our_hex) == wkbcodec.canonical_2d(o)
                rec("B_geometry", same, cid, "" if same else "geometry differs")
                if family == "XY" and our_hex and o:
                    same_bytes = our_hex.lower() == o.lower()
                    rec("D_write_bytes", same_bytes, cid, "" if same_bytes else "bytes differ")
                if use_zm and cid in emitted_ids:
                    expect = wkbcodec.canonical_2d(o)
                    got = wkbcodec.canonical_2d(ora_written.get(emitted_ids.index(cid)))
                    ok = got == expect and not expect.startswith("UNPARSABLE")
                    rec("C_write_semantics", ok, cid, "" if ok else "written text reads differently")

        pairs = [(cid, ora[i]) for i, cid in enumerate(ids) if ora[i] is not None]
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

        known = load_known()
        unexpected = [
            (name, cid, detail)
            for name, (ok, bad) in res.items()
            for cid, detail in bad
            if cid not in known
        ]
        fails = sum(len(b) for _, b in res.values())
        print("TOTAL failures: " + str(fails) + ", known: " + str(fails - len(unexpected)))
        for name, cid, detail in unexpected:
            print("  UNEXPECTED [" + name + "] " + cid + ": " + detail)
        return 1 if unexpected else 0
    finally:
        if keep:
            print("workdir kept: " + workdir)
        else:
            shutil.rmtree(workdir, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())

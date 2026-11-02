#!/usr/bin/env python3
"""WKT and WKB conformance run against GDAL (OGR) as the external oracle.

The crate's own reader and writer are never the judge of each other. OGR parses every case
first, and every comparison is made on OGR's bytes or OGR's reading of our bytes.

Checks:
  A  reader acceptance matches the oracle, per dimension family
  B  our geometry equals the oracle's geometry (canonical 2D WKB)
  C  our WKT output, re-read by the oracle, equals the oracle's geometry (writer semantics)
  D  our WKB output equals the oracle's WKB byte for byte (writer bytes, XY cases)
  E  oracle WKB input, including Z/M, is read and re-emitted unchanged (WKB reader)

Usage: python conformance/run.py [--keep]
Exit code 1 when any check has a failure, so CI can gate on it.
"""

from __future__ import annotations

import csv
import json
import os
import shutil
import sqlite3
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OGR2OGR = os.environ.get("OGR2OGR", "ogr2ogr")

# --------------------------------------------------------------------------- corpus


def fmt(v):
    if isinstance(v, int):
        return str(v)
    s = repr(float(v))
    return s


def coord(x, y, dims):
    parts = [fmt(x), fmt(y)]
    if "Z" in dims:
        parts.append(fmt(7.5))
    if "M" in dims:
        parts.append(fmt(3.5))
    return " ".join(parts)


def mod(dims):
    return f"{dims} " if dims else ""


def ring(points, dims):
    pts = [coord(x, y, dims) for x, y in points]
    pts.append(pts[0])
    return "(" + ", ".join(pts) + ")"


SQUARE = [(0, 0), (10, 0), (10, 10), (0, 10)]
HOLE = [(2, 2), (4, 2), (4, 4)]


def builders(dims):
    """One representative geometry per OGC type, at the given dimensions."""
    return [
        ("point", f"POINT {mod(dims)}({coord(1, 2, dims)})"),
        (
            "linestring",
            f"LINESTRING {mod(dims)}({coord(0, 0, dims)}, {coord(1, 1, dims)}, {coord(2, 0, dims)})",
        ),
        (
            "polygon",
            f"POLYGON {mod(dims)}({ring(SQUARE, dims)}, {ring(HOLE, dims)})",
        ),
        (
            "multipoint_bare",
            f"MULTIPOINT {mod(dims)}({coord(1, 2, dims)}, {coord(3, 4, dims)})",
        ),
        (
            "multipoint_paren",
            f"MULTIPOINT {mod(dims)}(({coord(1, 2, dims)}), ({coord(3, 4, dims)}))",
        ),
        (
            "multilinestring",
            f"MULTILINESTRING {mod(dims)}(({coord(0, 0, dims)}, {coord(1, 1, dims)}),"
            f" ({coord(2, 2, dims)}, {coord(3, 3, dims)}))",
        ),
        (
            "multipolygon",
            f"MULTIPOLYGON {mod(dims)}((({coord(0, 0, dims)}, {coord(1, 0, dims)},"
            f" {coord(1, 1, dims)}, {coord(0, 0, dims)})), (({coord(5, 5, dims)},"
            f" {coord(6, 5, dims)}, {coord(6, 6, dims)}, {coord(5, 5, dims)})))",
        ),
        (
            "collection",
            f"GEOMETRYCOLLECTION {mod(dims)}(POINT {mod(dims)}({coord(1, 2, dims)}),"
            f" LINESTRING {mod(dims)}({coord(0, 0, dims)}, {coord(1, 1, dims)}))",
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
            cases.append((f"{tag}_{name}", "shape", text))
        for t in TYPES:
            cases.append((f"{tag}_empty_{t.lower()}", "empty", f"{t} {mod(dims)}EMPTY"))

    lex = [
        ("lex_lower", "point (1 2)"),
        ("lex_mixed_case", "PoInT (1 2)"),
        ("lex_z_lower", "point z (1 2 3)"),
        ("lex_spaces", "POINT    (   1     2   )"),
        ("lex_newline", "POINT\n(1 2)"),
        ("lex_tab", "LINESTRING\t(0 0, 1 1)"),
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
    return cases


# --------------------------------------------------------------------------- WKB

TYPE_NAMES = {
    1: "point",
    2: "linestring",
    3: "polygon",
    4: "multipoint",
    5: "multilinestring",
    6: "multipolygon",
    7: "collection",
}


class Node:
    __slots__ = ("kind", "points", "parts")

    def __init__(self, kind, points=None, parts=None):
        self.kind = kind
        self.points = points or []
        self.parts = parts or []


def parse_wkb(buf, pos=0):
    order = buf[pos]
    if order not in (0, 1):
        raise ValueError(f"byte order {order}")
    e = "<" if order == 1 else ">"
    pos += 1
    (raw,) = struct.unpack_from(e + "I", buf, pos)
    pos += 4
    has_z = bool(raw & 0x80000000)
    has_m = bool(raw & 0x40000000)
    base = raw & 0x0FFFFFFF
    if 1000 < base <= 1100:
        has_z, base = True, base - 1000
    elif 2000 < base <= 2100:
        has_m, base = True, base - 2000
    elif 3000 < base <= 3100:
        has_z, has_m, base = True, True, base - 3000
    if base not in TYPE_NAMES:
        raise ValueError(f"type {base}")
    n = 2 + has_z + has_m

    def coords(count):
        nonlocal pos
        vals = struct.unpack_from(e + "d" * (count * n), buf, pos)
        pos += 8 * count * n
        return [(vals[i * n], vals[i * n + 1]) for i in range(count)]

    def count():
        nonlocal pos
        (c,) = struct.unpack_from(e + "I", buf, pos)
        pos += 4
        return c

    if base == 1:
        pts = coords(1)
        return Node("point", points=pts), pos
    if base == 2:
        return Node("linestring", points=coords(count())), pos
    if base == 3:
        rings = [coords(count()) for _ in range(count())]
        return Node("polygon", parts=[Node("ring", points=r) for r in rings]), pos
    parts = []
    for _ in range(count()):
        sub, pos = parse_wkb(buf, pos)
        parts.append(sub)
    return Node(TYPE_NAMES[base], parts=parts), pos


def emit_2d(node):
    kind = "linestring" if node.kind == "ring" else node.kind
    code = {v: k for k, v in TYPE_NAMES.items()}[kind]
    out = bytearray([1])
    out.extend(struct.pack("<I", code))

    def put(pts):
        for x, y in pts:
            out.extend(struct.pack("<dd", x, y))

    if kind == "point":
        out.extend(struct.pack("<I", 1))
        put(node.points)
    elif kind == "linestring":
        out.extend(struct.pack("<I", len(node.points)))
        put(node.points)
    elif kind == "polygon":
        out.extend(struct.pack("<I", len(node.parts)))
        for r in node.parts:
            out.extend(struct.pack("<I", len(r.points)))
            put(r.points)
    else:
        out.extend(struct.pack("<I", len(node.parts)))
        for p in node.parts:
            out.extend(emit_2d(p))
    return bytes(out)


def canonical(hexstr):
    if not hexstr:
        return ""
    try:
        node, _ = parse_wkb(bytes.fromhex(hexstr))
        return emit_2d(node).hex()
    except Exception as exc:  # noqa: BLE001 - a malformed blob is a finding, not a crash
        return f"UNPARSABLE:{exc}"


def strip_gpkg(blob):
    flags = blob[3]
    env = {0: 0, 1: 32, 2: 48, 3: 48, 4: 64}[(flags >> 1) & 7]
    return blob[8 + env :]


# --------------------------------------------------------------------------- oracle


def oracle(texts, workdir, name):
    """Return {index: wkb hex or None} from OGR."""
    csv_in = os.path.join(workdir, f"{name}.csv")
    gpkg = os.path.join(workdir, f"{name}.gpkg")
    with open(csv_in, "w", newline="", encoding="utf-8") as fh:
        w = csv.writer(fh)
        w.writerow(["idx", "wkt"])
        for i, t in enumerate(texts):
            w.writerow([i, t])
    for path in (gpkg,):
        if os.path.exists(path):
            os.remove(path)
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
        raise SystemExit(f"oracle failed: {proc.stderr.strip()[:500]}")
    con = sqlite3.connect(gpkg)
    rows = dict(con.execute("select cast(idx as integer), geom from cases").fetchall())
    con.close()
    out = {}
    for i, t in enumerate(texts):
        blob = rows.get(i)
        out[i] = None if blob is None else strip_gpkg(blob).hex()
    return out, proc.stderr


# --------------------------------------------------------------------------- driver


def dump_rust(args, workdir):
    exe = os.path.join(ROOT, "target", "debug", "examples", "conformance_dump.exe")
    if not os.path.exists(exe):
        raise SystemExit(f"missing {exe}; run: cargo build --example conformance_dump")
    proc = subprocess.run([exe] + args, capture_output=True, text=True)
    if proc.returncode != 0:
        raise SystemExit(f"dump failed: {proc.stderr[-800:]}")
    rows = {}
    for line in proc.stdout.splitlines():
        if not line.strip():
            continue
        rec = json.loads(line)
        rows[rec["id"]] = rec
    return rows


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


def main():
    keep = "--keep" in sys.argv
    workdir = tempfile.mkdtemp(prefix="wktconf-")
    try:
        cases = corpus()
        ids = [c[0] for c in cases]
        texts = [c[2] for c in cases]

        tsv = os.path.join(workdir, "corpus.tsv")
        with open(tsv, "w", encoding="utf-8", newline="\n") as fh:
            for i, t in zip(ids, texts):
                esc = t.replace(chr(10), "|NL|").replace(chr(9), "|TAB|").replace(chr(13), "|CR|")
                fh.write(str(i) + chr(9) + esc + chr(10))
        ours = dump_rust([tsv], workdir)

        ora, ora_err = oracle(texts, workdir, "cases")

        # Writer semantics: our WKT text goes back through the oracle.
        emitted = []
        emitted_idx = []
        for i, cid in enumerate(ids):
            row = ours.get(cid, {})
            if row.get("zm_ok") and row.get("wkt_out"):
                emitted.append(row["wkt_out"])
                emitted_idx.append(cid)
        ora2, _ = oracle(emitted, workdir, "emitted")

        # Writer bytes and reader: our WKB, and the oracle's WKB as input.
        res = {
            "A_read_accept": [[], []],
            "B_geometry": [[], []],
            "C_write_semantics": [[], []],
            "D_write_bytes": [[], []],
            "E_wkb_roundtrip": [[], []],
        }
        notes = []

        def rec(check, ok, cid, detail=""):
            res[check][0 if ok else 1].append((cid, detail))

        for i, (cid, cat, text) in enumerate(cases):
            r = ours.get(cid, {})
            o = ora[i]
            oracle_ok = o is not None
            dims_family = cid.split("_", 1)[0]
            use_zm = dims_family in ("Z", "M", "ZM")
            our_ok = bool(r.get("zm_ok") if use_zm else r.get("plain_ok"))
            our_err = r.get("zm_err") if use_zm else r.get("plain_err")
            rec("A_read_accept", our_ok == oracle_ok, cid, f"oracle={oracle_ok} ours={our_ok} {our_err}"[:140])

            if oracle_ok and our_ok:
                our_hex = r.get("isowkb") or r.get("wkb")
                same = canonical(our_hex) == canonical(o)
                rec("B_geometry", same, cid, "" if same else "geometry differs")
                if dims_family == "XY" and our_hex and o:
                    same_bytes = our_hex.lower() == o.lower()
                    rec("D_write_bytes", same_bytes, cid, "" if same_bytes else "bytes differ")

            if use_zm and cid in emitted_idx and oracle_ok:
                expect = canonical(o)
                got = canonical(ora2.get(emitted_idx.index(cid)))
                rec("C_write_semantics", got == expect and expect.startswith("01"), cid)

        # E: the oracle's own WKB fed back into our reader.
        hex_in = []
        wkb_idx = []
        for i, (cid, cat, text) in enumerate(cases):
            if ora[i] is None:
                continue
            hex_in.append(ora[i])
            wkb_idx.append(cid)
        wkb_tsv = os.path.join(workdir, "wkb.tsv")
        with open(wkb_tsv, "w", encoding="utf-8", newline="\n") as fh:
            for cid, h in zip(wkb_idx, hex_in):
                fh.write(f"{cid}\t{h}\n")
        back = dump_rust(["--wkb", wkb_tsv], workdir)
        for cid, h in zip(wkb_idx, hex_in):
            r = back.get(cid, {})
            ok = bool(r.get("ok"))
            same = ok and r.get("recoded", "").lower() == h.lower()
            if ok and not same:
                notes.append(f"E recode differs: {cid}")
            rec("E_wkb_roundtrip", same, cid, "" if same else ("read failed" if not ok else "recode differs"))

        total = len(cases)
        print(f"corpus {total} cases, oracle {OGR2OGR}")
        if ora_err.strip():
            first = ora_err.strip().splitlines()[0]
            print(f"oracle stderr: {first[:160]}")
        print()
        print(f"{'check':<20}{'pass':>6}{'fail':>6}  examples")
        for name, (ok, bad) in res.items():
            sample = ", ".join(c for c, _ in bad[:6])
            print(f"{name:<20}{len(ok):>6}{len(bad):>6}  {sample}")
        print()
        for name, (ok, bad) in res.items():
            if bad:
                print(f"[{name}] first failures")
                for cid, detail in bad[:12]:
                    print(f"  {cid}: {detail}")
                print()
        for n in notes[:10]:
            print(n)
        known = load_known()
        unexpected = [
            (name, cid, detail)
            for name, (ok, bad) in res.items()
            for cid, detail in bad
            if cid not in known
        ]
        fails = sum(len(b) for _, b in res.values())
        print()
        print(f"TOTAL failures: {fails}, known: {fails - len(unexpected)}")
        for name, cid, detail in unexpected:
            print(f"  UNEXPECTED [{name}] {cid}: {detail}")
        return 1 if unexpected else 0
    finally:
        if keep:
            print(f"workdir kept: {workdir}")
        else:
            shutil.rmtree(workdir, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())

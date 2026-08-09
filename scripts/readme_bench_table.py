#!/usr/bin/env python3
"""Synthetic-bench table sync + gate for README.md.

The README carries the FULL synthetic benchmark table - every row the
bench can emit. A trimmed "representative" table is a regression
(2026-08-09: the README had been cut to 18 of 91 rows; restored and
gated here so it cannot regress again).

Usage:
  python scripts/readme_bench_table.py --update bench.json   # regenerate the table from a settled full run
  python scripts/readme_bench_table.py --check  bench.json   # CI gate: every bench label must appear in the README table
  python scripts/readme_bench_table.py --readme PATH         # README to operate on (default README.md)

--check needs only the labels, so it works with any run (GEOS or not).
--update writes the GeoRepair / GEOS / Ratio columns from the run's
numbers; rows without a GEOS measurement get "-" in the GEOS column.
"""

import argparse
import json
import pathlib
import re
import sys

SECTION_START = "### Synthetic benchmarks"
SECTION_END = "### Run benchmarks"


def _read(path):
    data = pathlib.Path(path).read_bytes()
    crlf = b"\r\n" in data
    text = data.decode("utf-8").replace("\r\n", "\n")
    return text, crlf


def _write(path, text, crlf):
    if crlf:
        text = text.replace("\n", "\r\n")
    pathlib.Path(path).write_bytes(text.encode("utf-8"))


def load_rows(json_path):
    rows = []
    for line in pathlib.Path(json_path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def norm_label(label):
    # Bench labels are width-padded ("valid polygon    4v"); the README
    # table shows the clean form. Normalize both sides the same way.
    return " ".join(label.split())


def fmt_val(v):
    if v is None:
        return "-"
    if abs(v) >= 100:
        return f"{v:.0f}"
    if abs(v) >= 1:
        return f"{v:.1f}"
    s = f"{v:.3f}".rstrip("0").rstrip(".")
    return s if s not in ("", "-0") else "0"


def fmt_ratio(geos, ours):
    if geos is None or ours is None or ours <= 0:
        return "-"
    r = geos / ours
    if r >= 10:
        return f"{r:.0f}x"
    if r >= 1:
        return f"{r:.1f}x"
    return f"{r:.2f}x"


def _section_and_labels(text):
    m = re.search(re.escape(SECTION_START) + r"(.*?)" + re.escape(SECTION_END), text, re.S)
    if not m:
        return None, None
    labels = set()
    for line in m.group(1).splitlines():
        line = line.strip()
        if line.startswith("|") and not re.match(r"^\|[\s\-:|]+\|$", line):
            cells = [c.strip() for c in line.strip("|").split("|")]
            if cells and cells[0] != "Benchmark":
                labels.add(norm_label(cells[0]))
    return m, labels


def check(json_path, readme_path):
    rows = load_rows(json_path)
    bench_labels = {norm_label(r["label"]) for r in rows}
    text, _ = _read(readme_path)
    m, readme_set = _section_and_labels(text)
    if m is None:
        print(f"FAIL: README has no '{SECTION_START}' section")
        return 1
    missing = sorted(bench_labels - readme_set)
    if missing:
        print(f"FAIL: {len(missing)} bench row(s) missing from the README table:")
        for label in missing:
            print(f"  - {label}")
        print("Regenerate with: python scripts/readme_bench_table.py --update <bench.json>")
        return 1
    extra = sorted(readme_set - bench_labels)
    if extra:
        print(f"WARN: {len(extra)} README row(s) not produced by the bench (stale?):")
        for label in extra:
            print(f"  - {label}")
    print(f"OK: all {len(bench_labels)} bench rows present in the README table")
    return 0


def update(json_path, readme_path):
    rows = load_rows(json_path)
    text, crlf = _read(readme_path)
    # Lookahead: match the section body WITHOUT consuming the following
    # header, so text[m.end():] still starts at SECTION_END.
    m = re.search(re.escape(SECTION_START) + r".*?(?=" + re.escape(SECTION_END) + r")", text, re.S)
    if not m:
        print(f"FAIL: README has no '{SECTION_START}' section followed by '{SECTION_END}'", file=sys.stderr)
        return 1
    lines = [
        SECTION_START,
        "",
        "Full table, parallel batch (µs); ratio = GEOS / GeoRepair, >1 means",
        "we win. Methodology: `docs/BENCHMARKS.md`. Regenerate:",
        "`python scripts/readme_bench_table.py --update <bench.json>`; CI gate:",
        "`python scripts/readme_bench_table.py --check <bench.json>`.",
        "",
        "| Benchmark | GeoRepair | GEOS | Ratio |",
        "|-----------|----------:|-----:|------:|",
    ]
    for r in rows:
        ours = r.get("par_us")
        geos = r.get("geos_us")
        lines.append(
            f"| {norm_label(r['label'])} | {fmt_val(ours)} | {fmt_val(geos)} | {fmt_ratio(geos, ours)} |"
        )
    lines.append("")
    lines.append(SECTION_END)
    new_section = "\n".join(lines) + "\n"
    _write(readme_path, text[: m.start()] + new_section + text[m.end() :], crlf)
    print(f"OK: README table updated with {len(rows)} rows")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--update", metavar="bench.json", help="regenerate the README table from a bench JSON (NDJSON)")
    ap.add_argument("--check", metavar="bench.json", help="verify every bench label appears in the README table")
    ap.add_argument("--readme", default="README.md", help="README path (default README.md)")
    args = ap.parse_args()
    if bool(args.update) == bool(args.check):
        ap.error("exactly one of --update / --check is required")
    if args.update:
        return update(args.update, args.readme)
    return check(args.check, args.readme)


if __name__ == "__main__":
    sys.exit(main())

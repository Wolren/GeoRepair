#!/usr/bin/env python3
"""CI bench gate for geo-repair.

Runs the synthetic bench (no GEOS) on a curated case subset, compares the
parallel (batch) per-poly numbers against the committed baseline, and fails
when a case regresses past the threshold. The baseline is environment-bound
(recorded on the ubuntu-latest runner); when it is missing the run RECORDS
a new baseline and passes, so the first CI run establishes the numbers and
the artifact is committed by a maintainer.

Usage (from the repo root):
  BENCH_SUBSET=... BENCH_JSON=target/bench.json cargo bench \
      --features arrange,structure,parallel,simd --bench bench
  python scripts/bench_gate.py --json target/bench.json \
      --baseline benches/bench_baseline.json [--threshold 0.30]
"""
import argparse
import json
import os
import subprocess
import sys

# Curated regression-sensitive cases. The parallel column is the measured
# quantity; sub-multi-µs rows are excluded (Rayon dispatch noise).
CASE_PREFIXES = [
    "valid polygon 1000v",
    "valid polygon 5000v",
    "valid polygon 10000v",
    "invalid bowtie 4v",
    "invalid bowtie 50v",
    "invalid bowtie 100v",
    "invalid bowtie 500v",
    "spaghetti 500v",
    "self-touch poly",
    "valid ls 100v",
    "valid ls 500v",
    "collinear ls 500v",
    "duped ls 100v",
    "dense self ls 500v",
    "lissajous 2000v",
    "mls 50x3v",
    "self-int mls 50x4v",
    "hole hier 50h",
    "overlap mp 50sh",
    "dense grid 10x10=100",
    "dense grid 20x20=400",
    "sliver 500v",
]

# Absolute floor (µs) below which a case is not gated: sub-µs rows are
# dispatch-noise territory.
FLOOR_US = 1.0


def run_bench() -> str:
    env = dict(os.environ)
    env["BENCH_SUBSET"] = ",".join(CASE_PREFIXES)
    env["BENCH_JSON"] = "target/bench_gate.json"
    if os.path.exists("target/bench_gate.json"):
        os.remove("target/bench_gate.json")
    subprocess.run(
        ["cargo", "bench", "--features", "arrange,structure,parallel,simd", "--bench", "bench"],
        env=env,
        check=True,
    )
    with open("target/bench_gate.json", encoding="utf-8") as f:
        return f.read()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", default=None, help="path to the bench JSON (runs the bench if omitted)")
    ap.add_argument("--baseline", default="benches/bench_baseline.json")
    ap.add_argument("--threshold", type=float, default=0.30)
    args = ap.parse_args()

    raw = run_bench() if args.json is None else open(args.json, encoding="utf-8").read()
    rows = {}
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        rec = json.loads(line)
        rows[rec["label"]] = rec

    baseline_path = args.baseline
    if not os.path.exists(baseline_path):
        # Record mode: first run on this environment establishes the baseline.
        with open(baseline_path, "w", encoding="utf-8") as f:
            json.dump({k: v["par_us"] for k, v in rows.items()}, f, indent=2, sort_keys=True)
        print(f"BENCH-GATE: no baseline found - RECORDED {baseline_path} ({len(rows)} cases)")
        print("BENCH-GATE: pass (recording run)")
        return 0

    with open(baseline_path, encoding="utf-8") as f:
        baseline = json.load(f)

    failures = []
    for label, base in sorted(baseline.items()):
        rec = rows.get(label)
        if rec is None:
            print(f"BENCH-GATE: missing case {label!r} in this run (baseline only) - SKIP")
            continue
        cur = rec["par_us"]
        if cur < FLOOR_US and base < FLOOR_US:
            print(f"BENCH-GATE: {label}: {cur:.3f}us (baseline {base:.3f}us) - below floor, SKIP")
            continue
        limit = base * (1.0 + args.threshold) + 0.05
        if cur > limit:
            failures.append((label, base, cur, limit))

    ok = True
    for label, base, cur, limit in failures:
        ok = False
        print(
            f"BENCH-GATE: FAIL {label}: {cur:.3f}us vs baseline {base:.3f}us "
            f"(limit {limit:.3f}us, +{args.threshold:.0%})"
        )
    for label in sorted(rows):
        if label in baseline:
            continue
        print(f"BENCH-GATE: new case {label!r} not in baseline - add it to the baseline")
        ok = False

    if ok:
        print(f"BENCH-GATE: pass ({len(baseline)} cases within +{args.threshold:.0%})")
        return 0
    print("BENCH-GATE: FAILED - a benchmark regression was detected")
    return 1


if __name__ == "__main__":
    sys.exit(main())

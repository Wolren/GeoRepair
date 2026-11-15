#!/usr/bin/env python3
"""Check R: does RustGIS Repair Geometry keep the even-odd interior of the input?

Runs the RustGIS checkout named by `RUSTGIS_DIR` headlessly through its own public tool
dispatch, on a fixed set of reproducers, and compares the area it keeps with the even-odd
area of the input boundary (the same oracle check O uses). It reports; it does not fail the
gate. The tool uses the shells-minus-holes rule on purpose, so a divergence is a recorded
measurement, not a regression in this crate.

Skip: no `RUSTGIS_DIR`, no Repair Geometry at that checkout, no `shapely`, or no Rust
toolchain. The first run builds a probe crate and takes a few minutes; later runs reuse
`%TEMP%/georepair-rustgis-probe`.

Usage:
    RUSTGIS_DIR=/path/to/RustGIS python3 conformance/rustgis_repair_oracle.py [--strict]

`--strict` exits 1 when a case diverges, for a human who wants that; the conformance run
never passes it, so this check is not a gate.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

try:
    import shapely
    from shapely import wkt as shapely_wkt
except ImportError:
    shapely = None

# id, expected even-odd area from the oracle, WKT. The last two are controls both rules agree on.
CASES = [
    (
        "pentagram (self-overlap)",
        "POLYGON ((0 0, 2 3, 4 0, 0 2, 4 2, 0 0))",
    ),
    (
        "hole covers shell",
        "POLYGON ((30 70, 70 70, 70 30, 30 30, 30 70), (10 90, 90 90, 90 10, 10 10, 10 90))",
    ),
    (
        "hole outside shell",
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0), (20 20, 21 20, 21 21, 20 20))",
    ),
    (
        "degenerate shell with a hole",
        "POLYGON ((10 10, 10 90, 90 90, 10 90, 10 10), (20 80, 60 80, 60 40, 20 40, 20 80))",
    ),
    (
        "overlapping parts",
        "MULTIPOLYGON (((0 0, 2 0, 2 2, 0 2, 0 0)), ((1 1, 3 1, 3 3, 1 3, 1 1)))",
    ),
    (
        "control: valid square",
        "POLYGON ((0 0, 10 0, 10 10, 0 10, 0 0))",
    ),
    (
        "control: bowtie",
        "POLYGON ((0 0, 1 1, 1 0, 0 1, 0 0))",
    ),
]

PROBE_MAIN = r'''
use std::io::Read;

use rustgis_analysis::{Inputs, Output};
use rustgis_data::interop;
use rustgis_data::{Dataset, Field, FieldType, Geometry, GeometryType, Value};
use serde_json::json;

struct Layer {
    ds: Dataset,
}

impl Inputs for Layer {
    fn dataset(&self, name: &str) -> Option<&Dataset> {
        (name == "input").then_some(&self.ds)
    }
    fn selection(&self, _: &str) -> &[usize] {
        &[]
    }
}

fn kind(g: &Geometry) -> GeometryType {
    match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => GeometryType::Point,
        Geometry::Line(_) => GeometryType::Polyline,
        Geometry::Polygon(_) => GeometryType::Polygon,
    }
}

fn main() {
    let mut text = String::new();
    if std::io::stdin().read_to_string(&mut text).is_err() {
        println!("{}", json!({"error": "stdin unreadable"}));
        return;
    }
    let geom = match interop::parse_wkt_zm(text.trim()) {
        Ok((g, _srid, _zm)) => g,
        Err(e) => {
            println!("{}", json!({"error": format!("parse: {e}")}));
            return;
        }
    };
    let ty = geom.as_ref().map(kind).unwrap_or(GeometryType::Polygon);
    let mut ds = Dataset::new("input", ty, vec![Field::new("src", FieldType::Text)]);
    ds.push_next(geom, vec![Value::Text("x".into())]);
    let layer = Layer { ds };
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rustgis_analysis::run("repairGeometry", &layer, &json!({"input": "input"}))
    }));
    match out {
        Ok(Ok(r)) => match r.output {
            Output::Layer(d) => {
                let text = d
                    .features
                    .first()
                    .and_then(|f| f.geometry.as_ref())
                    .map(interop::wkt);
                println!("{}", json!({"output": text}));
            }
            _ => println!("{}", json!({"error": "output is not a layer"})),
        },
        Ok(Err(e)) => println!("{}", json!({"error": e})),
        Err(_) => println!("{}", json!({"error": "panic in repairGeometry"})),
    }
}
'''

PROBE_CARGO = """[package]
name = "rustgis-repair-probe"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
rustgis-analysis = {{ path = "{dir}/crates/analysis" }}
rustgis-data = {{ path = "{dir}/crates/data" }}
serde_json = "1"

[workspace]
"""


def skip(reason):
    print("check R skipped: " + reason)
    return 0


def build_probe(rustgis_dir):
    """The prebuilt probe from RUSTGIS_REPAIR_PROBE, or a crate built beside the temp folder."""
    given = os.environ.get("RUSTGIS_REPAIR_PROBE")
    if given:
        if os.path.exists(given):
            return given
        return None
    root = os.path.join(tempfile.gettempdir(), "georepair-rustgis-probe")
    crate = os.path.join(root, "probe")
    os.makedirs(os.path.join(crate, "src"), exist_ok=True)
    with open(os.path.join(crate, "Cargo.toml"), "w", encoding="utf-8") as fh:
        fh.write(PROBE_CARGO.format(dir=rustgis_dir.replace("\\", "/")))
    with open(os.path.join(crate, "src", "main.rs"), "w", encoding="utf-8") as fh:
        fh.write(PROBE_MAIN)
    env = dict(os.environ)
    env["CARGO_TARGET_DIR"] = os.path.join(root, "target")
    print("check R: building the probe against %s (first run takes a few minutes)" % rustgis_dir)
    proc = subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=crate, env=env, capture_output=True, text=True)
    if proc.returncode != 0:
        tail = (proc.stderr or "").strip().splitlines()[-3:]
        for line in tail:
            print("check R: " + line)
        return None
    exe = os.path.join(root, "target", "release", "rustgis-repair-probe.exe")
    if not os.path.exists(exe):
        exe = os.path.join(root, "target", "release", "rustgis-repair-probe")
    return exe if os.path.exists(exe) else None


def run_probe(exe, text):
    try:
        proc = subprocess.run([exe], input=text.encode(), capture_output=True, timeout=30)
    except (subprocess.TimeoutExpired, OSError):
        return None, "no answer"
    if proc.returncode != 0:
        return None, "probe exit %d" % proc.returncode
    try:
        return json.loads(proc.stdout.decode().strip().splitlines()[-1]), ""
    except (ValueError, IndexError):
        return None, "probe printed no JSON"


def main():
    from run import even_odd_area

    if shapely is None:
        return skip("shapely is not installed")
    rustgis_dir = os.environ.get("RUSTGIS_DIR")
    if not rustgis_dir:
        return skip("RUSTGIS_DIR is not set")
    if not os.path.exists(os.path.join(rustgis_dir, "crates", "analysis", "src", "tools", "repair_geometry.rs")):
        return skip("no Repair Geometry in %s (needs the #1443 merge or later)" % rustgis_dir)
    exe = build_probe(rustgis_dir)
    if exe is None:
        return skip("the probe could not be built (is cargo installed?)")

    width = max(len(name) for name, _ in CASES)
    print("check R: Repair Geometry against the even-odd oracle, %s" % rustgis_dir)
    print("%-*s %12s %12s %9s  %s" % (width, "case", "even-odd", "repaired", "delta", "verdict"))
    diverged = 0
    for name, text in CASES:
        input_geom = shapely_wkt.loads(text)
        want = even_odd_area(input_geom)
        out, why = run_probe(exe, text)
        if out is None or out.get("error"):
            print("%-*s %12.4f %12s %9s  error: %s" % (width, name, want, "n/a", "n/a", why or out.get("error")))
            diverged += 1
            continue
        got = shapely_wkt.loads(out["output"]).area if out.get("output") else 0.0
        delta = (got - want) / want if want else 0.0
        ok = abs(got - want) <= max(1e-6 * abs(want), 1e-9)
        diverged += 0 if ok else 1
        print("%-*s %12.4f %12.4f %8.1f%%  %s" % (width, name, want, got, 100.0 * delta, "ok" if ok else "DIVERGES"))
    print(
        "check R: %d of %d cases diverge. Repair Geometry applies shells-minus-holes, not the"
        " even-odd rule; see opengeos/RustGIS#2003. This check reports, it does not fail the gate."
        % (diverged, len(CASES))
    )
    return 1 if diverged and "--strict" in sys.argv else 0


if __name__ == "__main__":
    sys.exit(main())

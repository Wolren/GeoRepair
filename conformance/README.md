# OGC conformance harness

Judges this crate's WKT and WKB readers and writers against GDAL, an implementation we do not
own. The crate never reads its own output to decide whether that output is correct.

## Run

```
cargo build --example conformance_dump
python3 conformance/run.py
```

`OGR2OGR` overrides the binary when GDAL is not on `PATH`. Exit code 1 means an unexpected
failure. A failing case listed in `known_divergences.txt` with a reason does not fail the gate.

CI runs this in the `conformance` job on Ubuntu with `gdal-bin` installed.

## What each check proves

| check | proof |
|---|---|
| A read acceptance | our verdict matches OGR's, per case |
| B geometry equality | our geometry and OGR's agree in canonical 2D WKB |
| C write semantics | our WKT text, read back by OGR, gives OGR's geometry |
| D write bytes | our ISO WKB equals OGR's WKB byte for byte, 2D cases |
| E WKB round trip | OGR's bytes, including Z and M, read and re-emit unchanged |

Check E is what proves ISO 13249 type codes (1001, 2001, 3001) are read, not only the PostGIS
EWKB high-bit flags.

## Shape of the corpus

99 cases: every OGC type at XY, Z, M and ZM, EMPTY at every level, both `MULTIPOINT` forms,
nested collections, empty holes and empty parts, lexical variants, and cases that must be
rejected. The corpus lives in `run.py` as generators, so a case is added once.

## Oracle limits worth knowing

OGR is lenient where the specification is not. It accepts `POINT (1 2 3)` with no modifier,
`POINT Z (1 2)`, trailing junk, and a missing comma. Our reader rejects all four, so those
lines sit in `known_divergences.txt` with that reason. OGR also closes rings on ingest, which
we do not, so an unclosed ring is listed there too.

A divergence entry states which side is wrong. An entry that cannot name a reason is a bug,
not a divergence.

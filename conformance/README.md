# OGC conformance harness

Judges this crate's WKT and WKB readers and writers against GDAL, an implementation we do not
own. The crate never reads its own output to decide whether that output is correct.

## Run

```
cargo build --example conformance_dump
python3 conformance/run.py
```

`OGR2OGR` overrides the binary when GDAL is not on `PATH`. CI runs this in the `conformance`
job with `gdal-bin` installed.

The gate measures our verdict against the specification, never against OGR's tolerance. Each
case carries an expected verdict, and a mismatch fails. OGR is used only where it is a real
authority: for bytes, and for geometry.

The gate can fail: `CONFORMANCE_SELFTEST=1 python3 conformance/run.py` injects a geometry the
specification accepts and the reader cannot parse, and must exit 1. A gate that cannot fail is
not a gate.

## Checks

| check | proof | how |
|---|---|---|
| A read acceptance | our verdict matches the specification | corpus of 107 cases |
| B geometry equality | our geometry equals OGR's | canonical 2D WKB |
| C write semantics | our WKT, read by OGR, equals OGR's geometry | second oracle pass |
| D write bytes | our ISO WKB equals OGR's bytes | 2D cases, byte compare |
| E WKB round trip | OGR's bytes read and re-emit unchanged | all dimensions |
| F big-endian | big-endian bytes read and kept | encoder independent of the crate |
| G EWKB SRID | an EWKB SRID survives the read | SRID 4326 on every readable case |
| H damaged input | 458 truncations and broken headers never panic | every byte cut of six seeds |
| I nesting | collections nest, absurd depth fails cleanly | depth 2, 12, 64 |
| J curved types | SQL/MM types are refused, not misread | 8 curved and surface forms |

Check E is what proves ISO 13249 type codes (1001, 2001, 3001) are read, not only the PostGIS
EWKB high-bit flags. Check D is what proves we emit the ISO codes back.

## Coverage contract

Covered and gated: the seven OGC simple-feature types, at XY, Z, M and ZM; EMPTY at every
level; both `MULTIPOINT` forms; nested collections; empty holes and empty parts; both byte
orders; ISO and EWKB type-code forms; SRID on EWKB; truncated and malformed input.

Out of scope, with the reason stated: SQL/MM curved and surface types (`CIRCULARSTRING`,
`COMPOUNDCURVE`, `CURVEPOLYGON`, `MULTICURVE`, `MULTISURFACE`, `POLYHEDRALSURFACE`, `TIN`,
`TRIANGLE`). `geo` has no data model for them, so the reader refuses them with an unknown-type
error rather than inventing line segments. A caller with curved data must flatten it first.

Not covered yet, and therefore not claimed: CRS semantics of an SRID (we check the number
survives, not that it means a projection), the GeoPackage and CSV format layers themselves,
and precision round trips for values that need more than 17 significant digits.

## Rings and validity

The writers serialize the geometry they are given. An unclosed ring is written unclosed, which
is what GEOS does, and `POLYGON ((...), EMPTY)` is written with the empty ring dropped, because
WKT has no EMPTY inside a ring list. Validity is the job of the validator and `make_valid`, not
of the writer. OGR closes rings when it ingests them, so for that one case the byte comparisons are skipped:
they would compare our geometry with a normalised copy of it, which proves nothing.

## House rule

OGR is lenient where the specification is not. It accepts `POINT (1 2 3)` with no modifier,
`POINT Z (1 2)`, trailing junk, a missing comma, and the curved types. We reject all of these,
and the run prints that list as information, not as failures, because on those cases the
specification is on our side.

Two cases go the other way, and the run says so by name: `lex_plus` and `st_inf`. A leading
`+` is a signed numeric literal in the grammar; `Inf` is not, and we accept it anyway so
damaged corpora survive. That extension is deliberate and printed, not hidden in a list.

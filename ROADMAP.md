# Roadmap

GeoRepair exists to serve a host that cannot store invalid geometry. That host is RustGIS:
its WKT reader refuses EMPTY and NaN, it keeps only the first member of a collection, and its
geometry model is 2D. The work on this page follows that consumer.

This page is a stub: the sections stay, items enter when evidence exists, and no item carries a
date. The register of what the crate accepts and refuses is `conformance/COVERAGE.md`.

## Where the adapter stands

`src/host.rs` delivers to a closed 2D host. It flattens collections, drops non-finite coordinates
and empty parts, closes open rings, repairs when asked, and picks the type from what survives.
`host_clean` states the invariants a host may check for itself. `to_host_wkb` and `to_host_ewkt`
take bytes or PostGIS text in one call, and `src/interop.rs` does the same for any `geo-traits`
source.

Conformance check K measures the claim. A probe built from a RustGIS checkout reads the delivered
text and reports the same coordinates, 63 cases, compared as raw f64 bits. Four corpus cases are
unreadable by RustGIS raw and readable after the delivery pass: `st_mpoint_empty`, `st_nan`,
`st_inf`, `st_ninf`.

## Next

1. **Gate the consumer in CI.** Check K needs `RUSTGIS_DIR`. Without it the run builds no probe and
   prints `check K skipped`, which is how CI runs today. Pin a RustGIS revision and run the check
   nightly, so a delivery regression fails on the day it lands.
2. **Report every drop the way RustGIS reports it.** `HostReport` counts five kinds of loss. RustGIS
   forbids a silent drop, so the counts must reach `Session::notify`, one notice per kind.
3. **Keep Z when RustGIS keeps it.** Delivery drops Z and M because the host model is 2D. RustGIS
   keeps Z in phases 1 and 2 of #726. Delivery needs a Z mode before that lands, and check K
   compares Z at that point.

## Where the consumer work lands

These changes belong in the RustGIS repository, not here:

- a dependency on this crate on the load path;
- the `check and fix geometry` vector tool, which writes a new layer and carries the `HostReport`
  counts as notices;
- one undo step per run.

## Outside the adapter path

No priority is set. Each item names the evidence that put it here.

### Correctness

- **One area divergence from GEOS outside check N.** The 2000-vertex self-crossing walk fills
  212.74 where GEOS fills 790.49.
- **213 baselined XML cases diverge from GEOS**, mostly on the OGC winding contract
  (`docs/BENCHMARKS.md`).
- **No dimension inference from the ordinate count.** GEOS reads `POINT (1 2 3)` as Z; we refuse it
  because no modifier is present.
- **`fill_rule` is accepted and never read.** Repair applies EvenOdd, which matches GEOS on a
  pentagram (3.333 against NonZero's 4.667).
- **Z and M stay outside `Geometry`.** `WktGeometry::extra_coords` carries them.

### Performance

- **Pool saturation on giant-heavy batches.** The batch fills all 12 workers, so nested intra-poly
  rayon finds no idle thread (Amdahl-bound).
- **Giants above 4,096 ring edges route to the boolean pipeline** (`SP_MAX_EDGES` in
  `src/core/mod.rs`). Single-pass noding cost 168 ms against 36 ms on the 200k-edge case, which is
  why the route exists.
- **Mass-overlap repair is the slowest synthetic class**, about 1.5 ms for a 100-shell overlap.

### Bindings and packaging

- **Python tests cover the WKB and WKT surface only**, 41 tests in `tests/test_python.py`.
- **`proj` and `io-gpkg` cannot build together** because sqlite3 links once.

## Settled

- Curved types, TIN and polyhedral surfaces stay refused. `geo` has no data model for them.
- WKB output stays little-endian.
- No speed claim: the tables compare two crates on one corpus.

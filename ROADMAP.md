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

Z and M stay beside the geometry: `to_host_zm`, `to_host_wkb_zm` and `to_host_ewkt_zm` return a
`ZmGeometry`, and a coordinate that matches no source position within 1e-10 is counted in
`HostReport::lost_zm`.

`HostReport::notices` returns one `HostNotice` per change that occurred, each with its count. The
six kinds are in `HostChange`.

Conformance check K measures the claim. A probe built from a RustGIS checkout reads the delivered
text and reports the same coordinates, 65 cases, compared as raw f64 bits. Four corpus cases are
unreadable by RustGIS raw and readable after the delivery pass: `st_mpoint_empty`, `st_nan`,
`st_inf`, `st_ninf`.

The nightly `conformance-consumer` job runs that check against a pinned RustGIS commit, with
`CONFORMANCE_REQUIRE_K` set, so a skipped consumer fails the job. The pin moves by a pull request
that edits `RUSTGIS_PIN` in `.github/workflows/nightly.yml`.

## Next

The adapter side is in place. What remains is the consumer wiring, and it lands in the RustGIS
repository:

1. **Depend on the crate on the load path.** RustGIS has no `geo-repair` dependency today.
2. **Ship the `check and fix geometry` vector tool.** It writes a new layer and turns each
   `HostNotice` into `Session::notify`, so no drop is silent.
3. **Run the tool as one undo step.** The repair call is one command, so one undo entry covers it.
4. **Keep Z when RustGIS keeps it.** RustGIS adds Z in phases 1 and 2 of #726. Its WKT parser then
   needs a Z path, and check K compares Z at that point.

## Outside the adapter path

No priority is set. Each item names the evidence that put it here.

### Correctness

- **213 baselined cases in the reference XML corpus diverge**, mostly on the OGC winding contract
  (`docs/BENCHMARKS.md`).
- **No dimension inference from the ordinate count.** The reference reads `POINT (1 2 3)` as Z;
  we refuse it because no modifier is present.
- **`fill_rule` is accepted and never read.** Repair applies EvenOdd, which matches the reference
  on a pentagram (3.333 against NonZero's 4.667).
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

# GeoRepair Architecture

This document describes how GeoRepair actually works: the repair pipeline,
the routing rules that decide which path an input takes, the validation
strictness model, the error contract, and the feature surface. It is the
map for maintainers; code comments carry the per-site detail.

## 1. The repair contract

`make_valid` (and every dispatch arm it contains) guarantees:

1. **Valid or empty.** A repair output is either valid per our own
   validator (which agrees with GEOS on the OGC definition, see
   section 3) or an empty `GeometryCollection`. No arm may ship geometry
   the validator rejects. This is enforced by the gated fallback chain
   (`arrange_chain` in `src/make_valid/polygon.rs`) plus per-arm gates.
2. **No panics.** The foreign boolean overlay path (`i_overlay` via
   `geo::BooleanOps`) can assert on degenerate input; every repair
   dispatch site is wrapped in `std::panic::catch_unwind` and degrades to
   empty on a panic.
3. **NaN/Inf are filtered, not errors.** NaN/Inf coordinates are removed
   before dispatch (the exterior scan in `make_valid_with_config`); a
   polygon whose coordinates are entirely non-finite degrades to empty.
4. **Winding is normalized.** Output shells are CCW, holes CW
   (`enforce_ogc_winding` runs at every dispatch exit).

The upstream guarantee the crate is measured against: GEOS `makeValid`
accepts every GeoRepair repair output on the real-world dataset
(full-mode bench; the non-winding invalid class there is 1 polygon as of
2026-08-03, see section 3).

## 2. The repair pipeline

An invalid polygon walks a ladder of increasingly conservative paths.
Each path is tried in order; the first valid result wins.

```
input
  │
  ├─ fast path        valid input → passthrough (zero-copy, ~99.85% of
  │                   real-world data). Gates: ≥4 coords, basic form,
  │                   no sub-ULP edges, no proper crossings, inclusive
  │                   hole validity.
  │
  ├─ Structure single-pass   (structure/symdiff.rs)
  │   node shell + holes in ONE pass, walk even-odd faces (BuildArea).
  │   Primary path for self-crossings, crossing holes, hole overlaps.
  │   Result is OGC-wound and validated; failure falls through.
  │
  ├─ Structure boolean pipeline   (structure/, i_overlay subtract)
  │   per-ring symdiff + subtract_holes + merge. Safety net for
  │   single-pass failures and giants (> SP_MAX_EDGES = 4096 ring
  │   edges route here directly: single-pass noding is slower on
  │   huge rings).
  │
  ├─ Arrange CDT   (arrange/, spade triangulation at full f64 precision)
  │   node + face walk without any snap grid. Required for inputs the
  │   snap grid cannot represent (see routing rules).
  │
  ├─ Precision ladder   (reduce_fallback)
  │   re-run the repair at coarser grid scales (1e-10 … 1e-4). Handles
  │   near-collinear slivers that defeat exact arithmetic.
  │
  └─ empty GeometryCollection
```

### 2.1 Routing rules

- **`snap_cannot_represent(poly)`** (in `src/make_valid/polygon.rs`):
  the Structure single-pass and the boolean pipeline snap coordinates to
  the `SNAP_SCALE = 1e8` grid. If the polygon's smallest coordinate maps
  below 0.5 grid units (`min_abs * 1e8 < 0.5`) the snap destroys
  micro-features; if the largest exceeds 2^53 grid units, integer keys
  lose precision. Either condition routes the input away from Structure
  to the Arrange CDT path, which nodes at native f64. Measured on
  differential fuzz: mixed-magnitude polygons (1e-9 .. 1e7) repaired via
  the snapping paths produced self-intersecting output and i_overlay
  panics; the CDT path handles them.
- **`SP_MAX_EDGES`**: giants route to the boolean pipeline (see above).
- **Crossing holes** (a hole vertex strictly outside the shell): route to
  the boolean pipeline; the single-pass and the polygonizer both produce
  artifacts for them.
- **The `--fast` bench gate** uses the lightweight
  `arrange::validate_polygon` (orientation-agnostic), which is *stricter*
  than the full validator on real-world Structure outputs. The full
  Shewchuk validator is the canonical verdict; see section 3.

### 2.2 The gated fallback chain

Every dispatch arm funnels its failure path through `arrange_chain`:
Arrange's CDT output is OGC-wound, then gated by `is_valid_with_geo`;
on failure the precision ladder runs and is gated the same way; if both
fail, the result is empty. This is what makes "valid or empty" hold by
construction rather than by hope.

The Structure arm winds *before* its gate: the fast path can pass a
wrong-wound (CW) input through, and CW shells are valid per GEOS but
flagged `WrongOrientation` by our validator. Gating pre-winding would
send every CW passthrough to Arrange, which decomposes boundary-touching
holes into MultiPolygons (measured: `speed_bug_regressions`
`large_valid_gate_accepts_boundary_touching_hole`).

## 3. Validation model

The validator (`src/validation/`) runs Shewchuk exact predicates
(`orient2d` via the `robust` crate) and agrees with GEOS on the OGC
definition (934/934 GEOS XML suite cases pass). On top of exact
predicates it applies one **deliberate strictness gate**: edges whose
exact orientation is nonzero but within ~32 ulps of the pair's own
length scale (`32 * EPSILON * L²`) are treated as coincident. Measured
on the 1.58M real-world dataset (2026-08-03): 1,579,029 parts are
winding-only (CW, GEOS-valid) and exactly 1 carries a non-winding
defect: the gate's real-world impact is ~1 polygon. Earlier counts of
2,298 (via `arrange::validate_polygon`) and 1,855 (via the full
validator) were inflated by a genuine bug: the product-form
proper-crossing test `o1 * o2 < 0.0` treated a -0.0 orientation (an
exact collinear touch, common on snapped real-world vertices) as a
crossing. The zero-safe strict opposite-sign predicate
(`(o1 > 0 && o2 < 0) || (o1 < 0 && o2 > 0)`) now used by
`edges_intersect_general` and by the sweep
(`segments_properly_cross`, `has_no_intersections`) removed the false
positives and, together with an inclusive hole-containment check in
`arrange::validate_polygon`, closed the `--fast` bench gate artifact
(Structure outputs were flagged 2,298/2,298 while GEOS accepted them;
now the light gate agrees with GEOS). The crate still ships with the
strictness gate because production data's precision floor is far below
f64 and accepting noise-scale separations destabilizes downstream
overlay/buffer geometry.

The repair contract is a superset: GEOS `isValid` accepts every
repaired output the pipeline ships, and our stricter validator accepts
every repaired output it rejected the input for (the dispatch gates
guarantee valid-or-empty).

Validator gaps closed by differential fuzz vs GEOS (2026-08-03):

- **Exact-collinear micro-edge overlap** below the length gate: two
  edges bit-exactly on the same line whose length is below `eps` were
  skipped; now flagged regardless of scale (shared sub-grid topology is
  a deliberate touch, not rounding noise).
- **-0.0 pinches**: vertex deduplication now keys on
  `(x.to_bits(), y.to_bits())` so `(-0.0, 0.0)` and `(0.0, 0.0)` are the
  same pinch.
- **Closing-edge backtracking**: the pair `(0, n-1)` (the closing edge
  vs the first edge) was skipped outright; the two edges share vertex 0
  but can overlap collinearly beyond it. `edges_intersect_general`
  already excludes endpoint-only touches, so valid rings are unaffected.
- **Segment-local vertex-on-edge tolerance**: `point_strictly_on_segment`
  previously used the pair's larger edge length, inflating the
  strict-interior margin past micro segments in mixed-magnitude rings
  (a 2.3e6 edge made a 3e-8 closing edge's check vacuous). The tolerance
  is now computed from the tested segment itself.

## 4. Error model

| Input condition | Behavior |
|---|---|
| NaN/Inf coordinate | Filtered before dispatch; all-NaN input → empty |
| Zero-area / fully-collinear ring | Empty (collapsed); `keep_collapsed` preserves as LineString/Point |
| Degenerate ring (< 4 coords) | Point/LineString (deduped), or empty |
| Panic inside i_overlay | `catch_unwind` → empty + warning log |
| Unresolvable after all paths | Empty `GeometryCollection` |

`keep_collapsed` semantics mirror GEOS `MakeValid`'s keepCollapsed flag:
a collapsed ring is preserved as a lower dimension (LineString for a
collinear ring, Point for a single distinct point) instead of being
dropped.

## 5. Feature surface

| Feature | Enables | Notes |
|---|---|---|
| `arrange` | spade CDT repair path | implies `structure` (coupled pair) |
| `structure` | single-pass + boolean pipeline | implies `arrange` |
| `validate` | validator (rstar acceleration) | |
| `parallel` | rayon batch repair | |
| `simd` | scalar auto-vectorized kernels | hand-written AVX2 removed: measured slower than auto-vectorized scalar |
| `simd-portable` | `core::simd` | nightly-only (E0554 on stable, expected) |
| `geo-traits` | georust `GeometryTrait` interop | `interop.rs` + `validation/geo_bridge.rs` |
| `ffi` | C API + Python bindings (pyo3) | `VERSION_CSTR` derives from `CARGO_PKG_VERSION` |
| `io-*` | file formats (geojson, wkt, wkb, csv, gpkg, gml, shp) | `io-gpkg` + `proj` mutually exclusive (sqlite link conflict) |
| `serde` | config/geometry serialization | |
| `bench-geos(-system)` | GEOS comparison benches | needs system GEOS for `-system` |
| `mimalloc` | allocator (default) | |

No `std` feature means alloc-only: WKB parsing, validation, and repair
work without `std`; only file/network I/O requires it. The no_std CI job
proves the bare and validate-only configurations compile.

## 6. Benchmark methodology

- `benches/real_world.rs` is the canonical measurement: full pass over
  the 1.58M-polygon dataset (~3.9s, 0.98x vs GEOS), invalid-subset
  comparison vs GEOS, validation speed. `--fast` skips the GEOS
  comparison sections (~10s wall).
- Always take the **settled second run** (first-run-after-build is
  inflated ~18% by Windows Defender + cold LTO).
- Never trust a bench binary you cannot trace to a source file; rebuild
  explicitly after source changes.
- `examples/sp_diag.rs` is the invalid-subset diagnostic: acceptance
  counts + per-stage timing on the biggest giant.
- The GEOS comparison runs against conda-forge MSVC GEOS (serial
  per-call, no LTO); a static LLVM-built GEOS would improve the GEOS
  side of every table.

## 7. Fuzzing

`fuzz/` holds libFuzzer targets (cargo-fuzz, nightly):

- `make_valid`: raw coordinate streams → all three strategy modes; the
  output must be valid in every mode (valid-or-empty contract) and
  no panic may escape the containment guards.
- `validate`: the validator must not panic on arbitrary input; a polygon
  we call valid must not be collapsed by repair.
- `wkt_repair`: WKT parse + repair, exercising the IO layer with
  structured inputs (holes, MultiPolygons, degenerate rings).

The committed corpus (`fuzz/corpus/`) seeds the interesting classes:
bowties, mixed magnitudes, collinear spikes, crossing holes, NaNs. CI
runs a bounded smoke (`-runs=3000` per target); long local runs are the
way to hunt new crashes (`cargo +nightly fuzz run make_valid`).

Note: `cargo fuzz build` fails to LINK on Windows MSVC for this crate
because the lib is `crate-type = ["cdylib", "lib"]` (the C FFI needs the
cdylib) and the MSVC linker demands a `main` entry in the DLL under the
fuzz harness. This is a known cargo-fuzz + cdylib conflict on Windows;
the fuzz gate runs on Linux (CI), and locally the targets can be
type-checked with `cargo +nightly check --manifest-path fuzz/Cargo.toml --bins`.

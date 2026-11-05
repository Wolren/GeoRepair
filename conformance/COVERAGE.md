# Coverage register

What the readers, writers and the delivery adapter handle, and what they refuse. Every row names
its evidence: a conformance check (`A` to `N`, see `README.md`), a unit test, or a probe against
GDAL 3.9.2. A row without evidence does not belong here.

Specification basis: OGC 06-103r4 (WKT 1.2.1) and ISO 13249-3 (WKB). PostGIS EWKB is listed
because consumers send it, not because the specification requires it.

The oracle is GDAL 3.9.2. Where GDAL and the specification disagree, the specification wins and
the divergence is named below.

## WKT reader

| Surface | Status | Evidence |
|---|---|---|
| Seven types, nested `GEOMETRYCOLLECTION` | Accepted | A 111, B 87, I |
| `EMPTY` at every level | Accepted | A, B (31 empty cases) |
| `Z`, `M`, `ZM` separated from the type name | Accepted | A, D, E |
| Modifier welded to the type name (`POINTZ`, `MULTIPOLYGONZM`) | Accepted | A (4 cases), unit test |
| Any case (`point`, `PoInT`) | Accepted | `lex_lower`, `lex_mixed_case` |
| Space, tab, LF and CRLF between tokens | Accepted | `lex_newline`, `lex_tab`, probe |
| Scientific notation, `.5`, `5.`, `+1`, 20-significant-digit input | Accepted | `lex_exp`, `lex_leading_dot`, `lex_trailing_dot`, `lex_plus`, `lex_long` |
| `NaN`, `Inf`, `-Inf` keywords | Accepted, extension | `st_nan`, `st_inf`, `st_ninf` |
| Number above `f64` range (`1e400`) | Accepted as infinity | probe |
| Hex float (`0x1p3`) | Refused | probe; WKT has no hex form |
| Missing comma, wrong arity, trailing text, unbalanced parentheses | Refused | 10 `bad_*` cases |
| Collection whose members disagree on dimension | Refused | `st_coll_mixed_dims_bad` |
| Curved and surface types (`CIRCULARSTRING` ... `POLYHEDRALSURFACE`) | Refused | J 8 cases |
| Three ordinates with no modifier | Refused | `bad_arity_high` |
| `SRID=<n>;` prefix (EWKT) | Accepted by `read_ewkt`, SRID returned, outside OGC | 6 unit tests |
| Nesting depth | 12 deep accepted, depth 64 refused cleanly | I |

Two of these are declared leniencies: we accept `lex_plus` and `st_inf`, GDAL rejects both.
One is a declared strictness: we refuse three ordinates without a modifier, GDAL infers the
dimension. A refusal never invents a coordinate, which is the reason for the choice.

## WKT writer

| Surface | Status | Evidence |
|---|---|---|
| Canonical text, shortest round-trip number (`ryu`) | Correct | C 49, `src/io/wkt/write.rs:53` |
| `EMPTY` for every empty type | Correct | C, 3 unit tests |
| Empty ring dropped from a ring list, never printed as `EMPTY` | Correct | C, `polygon_with_empty_hole_drops_the_ring` |
| `Z`, `M`, `ZM` output | Correct | `write_wkt_zm`, E round trip |
| Ring closure | Reader closes an open ring; writer emits what it holds | measured in `st_unclosed_ring` |
| Locale | Never affected: `ryu` writes `.` regardless of locale | code path |

## WKB reader

| Surface | Status | Evidence |
|---|---|---|
| ISO 13249 type codes 1 to 7 | Accepted | E 87 |
| ISO 1000, 2000 and 3000 offsets, nested | Accepted | E, D (`Z_multipolygon` = 3003) |
| EWKB high-bit flags with SRID | Accepted | G 87 |
| SRID together with Z, M and ZM | Accepted | probe 4 of 4 |
| Both byte orders | Accepted | F 87 |
| Empty geometry as NaN ordinates | Accepted | E, probe (`iso_z_empty`, `iso_m_empty`) |
| Damaged, truncated and hostile bytes | Clean error, no panic, no hang | H 458 |
| Curved and SQL/MM codes 8 to 17 | Refused | J 8 |
| Nesting depth | Guarded | I |

## WKB writer

| Surface | Status | Evidence |
|---|---|---|
| EWKB with SRID, ISO without | Correct | D 86, G |
| Byte identical to GDAL, XY to ZM | Correct | D 86 of 86 |
| Empty Z and M as NaN ordinates | Correct | D (`POINT Z EMPTY` bytes) |
| Big-endian output | Not offered; the writer emits little-endian | by design |

## Delivery adapter

| Surface | Status | Evidence |
|---|---|---|
| Six output types, never a collection | Guaranteed | unit test, `host_clean`, K/L |
| Drops non-finite coordinates and degenerate parts, counts them | Guaranteed | `HostReport`, 10 tests |
| Closes an open ring, moves no coordinate | Guaranteed | unit test |
| Type chosen from survivors | Guaranteed | `the_type_follows_the_survivors` |
| Idempotent | Guaranteed | 500 generated shapes |
| Output accepted by RustGIS and the georust `wkt` crate | Verified | K 58, L 58 |
| Repair of invalid points, lines and polygons on request | Guaranteed | `HostOptions::REPAIR`, 8 tests |
| Polygon ring with one or two points | Refused | `WkbError::InvalidRing`, 11 of 135,494 fuzzed blobs |
| Ring of three points | Accepted | GDAL round trips it, `st_degenerate_ring` |
| Multipoint with duplicate coordinates | Deduplicated | GEOS 3.13.1 keeps both and calls the result not simple; M requires simple output |
| A part `make_valid` consumes without a replacement | Counted | `dropped_unrepairable`, `repair_reports_a_polygon_it_cannot_fix` |
| Repair never deletes what it cannot improve | Guaranteed | N, fallback in `to_host_with`, 6 adapter tests |
| A geometry GEOS calls valid and simple is unchanged by repair | Verified | N, 64 cases |
| Repaired output keeps area, length and point count against GEOS | Verified | N, 64 cases, no wider fill |
| Repair at collection level, so overlapping valid parts are fixed | Guaranteed | `repair_mode_fixes_overlapping_polygons`, M |
| Repaired output valid and simple per GEOS | Verified | M 62 delivered geometries, 5 repaired |
| Repair does not touch a valid polygon | Guaranteed | test `repair_mode_does_not_touch_a_valid_polygon` |
| Bytes in, host shape out (`to_host_wkb`, `to_host_ewkt`) | Guaranteed | 2 tests |

## Measured cost

Corpus: 813 IUCN range multipolygons, flattened to 1 579 030 polygons and 47 121 977 exterior
coordinates (774 MB). Reading that GeoPackage with our own WKB reader takes 0.87 s.

`cargo bench --bench wkb_cmp` against the georust `wkt` and `wkb` crates, per geometry:

| Operation | This crate | georust | Ratio |
|---|---|---|---|
| WKB read | 92 ns | 274 ns | 2.96x |
| WKB write | 48 ns | 74 ns | 1.53x |
| WKB round trip | 130 ns | 340 ns | 2.61x |
| WKT read | 2 080 ns | 5 097 ns | 2.45x |
| WKT write | 1 675 ns | 5 568 ns | 3.32x |
| WKT round trip | 10 037 ns | 32 885 ns | 3.28x |

`cargo bench --bench host_adapter` on the same corpus: `to_host` costs 142 ns per polygon in the
last clean run (117 to 142 across runs), against 66 ns for a bare clone and 186 ns for the older
`apply_policy` then `sanitize` chain. `HostOptions::REPAIR` costs 7 640 ns per polygon on the
densest 50 000, 54 times a plain reduce: validity checking dominates, and no coordinate moves
unless the check fails.

These are single-machine numbers with no allocator isolation. They compare implementations, and
they do not claim a limit on what any of them could reach.

## Not claimed

- Z and M live beside the geometry in `WktGeometry::extra_coords`, not inside `Geometry`.
- WKT carries no SRID. EWKT is a separate entry point, `read_ewkt` and `write_ewkt`, because GDAL refuses the prefix.
- No curved types, no TIN, no polyhedral surface. `geo` has no data model for them.
- Validity beyond closure is not this crate's claim. `make_valid` and the validator own it.
- No claim about digits beyond what `f64` holds. `ryu` writes the shortest exact round trip.
- No claim of being the fastest implementation. The table compares two crates on one corpus.
- `fill_rule` on `MakeValidConfig` is accepted and never read: repair always applies EvenOdd, which matches GEOS on a pentagram (both 3.333). The NonZero result, 4.667 with the centre filled, is not reachable.
- Repair cancels an overlapping or nested part as GEOS does: two 2x2 squares overlapping by 1 give area 6, not the union's 7.
- A self-crossing ring is not claimed to match GEOS in area outside check N: the 2000-vertex walk in the symdiff test converges to 212.74 where GEOS fills 790.49.

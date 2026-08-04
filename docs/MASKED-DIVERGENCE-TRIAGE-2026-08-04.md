# Masked Divergence Triage - 2026-08-04

The GEOS XML suite reports **210 masked divergences** (209 live XML cases + 1
ST_RFH corpus case): inputs GEOS deems VALID that our validator rejects, but
where repair restores validity AND preserves even-odd area (the mask gate).

This pass adds per-case identification (`DIAG_MASKED=1` env flag prints
`MASKED\t<file>\t<case>\t<error-class>` per case) and buckets every masked
case by the first validator error class.

## Class distribution (210 total)

| Class | Count | Meaning |
|---|---|---|
| WrongOrientation | 191 | Shells/holes with non-OGC winding (CW shells) |
| RepeatedPoint | 8 | Consecutive duplicate vertices in a ring |
| MultiPointDuplicatePoints | 8 | Duplicate members in a MultiPoint |
| RingTooFewPoints | 2 | Rings with fewer than 4 points |
| PinchPoint | 1 | Ring self-touch at a vertex |

## Verdict: zero genuine topological gaps

Every masked case is the **documented stricter-validator divergence** - our
validator is deliberately stricter than GEOS on classes GEOS's IsValidOp
ignores (orientation is not part of OGC validity; GEOS also tolerates
repeated/too-few points and MultiPoint duplicates). None of the 210 cases
represents a case where GEOS finds a real defect that we miss.

- **WrongOrientation (91%)** is load-bearing, not a bug: the repair dispatch
  routes on validator errors, and CW passthrough without the OGC rewind
  misroutes to arrange (documented pitfall, 2026-08-03 maturity pass). The
  validator's orientation strictness is what makes `enforce_ogc_winding`
  ordering safe. Making the validity REPORT orientation-agnostic would break
  the repair gates.
- **RepeatedPoint / MultiPointDuplicatePoints / RingTooFewPoints /
  PinchPoint (19 total)** are strictness choices: the repair strips these
  degeneracies; GEOS tolerates them in validity.
- The **area gate** (even-odd area preservation, 1e-6 relative) is the real
  correctness contract on the masked path: a repair that destroys coverage
  FAILS, it never masks. The island-in-hole nesting misclassification
  (-3.75% area, 2026-08-01) is caught by exactly this gate.

## Triage artifacts

- Per-case dump: `DIAG_MASKED=1 cargo test --test geos_xml_suite -- --nocapture`
- Representative cases: `misc_hexwkb.xml` (HEXWKB MultiPoint duplicate),
  `general_TestValid2.xml` Tests 72-99 (orientation), `issue_issue-geos-275.xml`
  (ticket 275 orientation), `misc_Segfaults.xml` (orientation).

## Recommendation

No action. The masked set is a stable, classed, drift-checked strictness
baseline - not correctness debt. Revisit only if the validator's orientation
policy changes (e.g. an opt-in GEOS-lenient validity mode), in which case the
191 WrongOrientation cases are the first place to look.

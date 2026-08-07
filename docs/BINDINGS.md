# Bindings: Python and C

geo-repair ships two language bindings alongside the Rust crate. Both
wrap the same engine and the same semantics: validate, repair, and
validate-and-fix over WKB and WKT, single geometry and batch.

- **Python** (`geo_repair` package, PyO3) — published on PyPI as
  `geo-repair`, abi3 wheels covering Python 3.8+.
- **C** (`geo_repair.h` FFI, WKB + WKT) — built as `cdylib` and
  `staticlib`, artifacts shipped with every GitHub release.

Both bindings are version-locked to the crate version
(`geo_repair.version()` and `geo_repair_version()` return the crate
version).

---

## Python bindings

### Install

```bash
pip install geo-repair
```

Wheels are `cp38-abi3` (one wheel per platform serves Python 3.8+). The
`parallel` feature is always enabled in published wheels; the wheel also
ships typing stubs (`geo_repair/__init__.pyi` + `py.typed`).

### Quickstart

```python
import geo_repair

# WKB (native GIS format, e.g. from QGIS/GDAL)
fixed = geo_repair.repair_wkb(wkb_bytes, method="auto", keep_collapsed=False)
ok, errors = geo_repair.validate_wkb(wkb_bytes)

# WKT (text)
fixed = geo_repair.repair_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")
was_valid, errors, fixed = geo_repair.validate_and_fix_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))")

# Batches (the parallel variants use the rayon batch internally)
results = geo_repair.par_repair_wkb_batch(list_of_wkb_bytes)

print(geo_repair.version())
```

### API surface

All functions exist for both WKB (`bytes`) and WKT (`str`), with the
suffixes `_wkb` / `_wkt`.

| Function | Returns | Semantics |
|---|---|---|
| `repair_*` | geometry | Repair, always valid-or-empty output. `method` and `keep_collapsed` accepted. Parse failure of a single input raises `ValueError`. |
| `repair_*_batch` | list of geometries | Per-input repair; unparseable inputs are returned **unchanged** (batch never fails as a whole). |
| `par_repair_*_batch` | list of geometries | Parallel batch; identical output to `repair_*_batch`. |
| `repair_validate_*` | `(fixed, was_valid, errors)` | Repair + validation report in one call. |
| `repair_validate_*_batch` | list of `(fixed, was_valid, errors)` | Batch form; parse failures report as `(input, False, [error])`. |
| `is_valid_*` | `bool` | OGC validity check. |
| `is_valid_*_batch` | list of `bool` | Batch form; parse failures report `False`. |
| `validate_*` | `(bool, [errors])` for WKB, `[errors]` for WKT | Errors are human-readable violation strings; collection children are prefixed `[geom N]`. |
| `validate_*_batch` | list of results | Batch form; parse failures report as error strings. |
| `validate_and_fix_*` | `(was_valid, errors, fixed)` | Validate, report, then repair. |
| `validate_and_fix_*_batch` | list of results | Batch form. |
| `version()` | `str` | Crate version (also `__version__`). |

Shared parameters:

- `method`: `"auto"` (default), `"arrange"`, or `"structure"` — the
  repair strategy (Structure mirrors GEOS ST_MakeValid; Arrange is the
  CDT-based fallback for complex topologies).
- `keep_collapsed`: `bool` — when True, collapsed (zero-area) components
  are kept instead of dropped.

Note the WKT/WKB asymmetry in `validate_*`: WKB returns
`(is_valid, errors)` while WKT returns only `[errors]` (an empty list
means valid). This is intentional and stable.

### QGIS

`qgis/qgis_geo_repair.py` is a complete QGIS Processing script using the
WKB batch surface: features are streamed from QGIS, chunked (500 per
batch), repaired by the Rust engine, and written back. Memory is O(1),
the progress bar stays live, and cancellation works mid-batch.

Install: copy the script (and the wheel, or `pip install geo-repair`)
into `%APPDATA%/QGIS/QGIS3/profiles/default/processing/scripts/`
(Windows) or `~/.local/share/QGIS/...` (Linux), restart QGIS, and the
"Geo Repair" algorithm appears in the Processing Toolbox.

---

## C bindings

### Build

```bash
cargo build --release --features ffi
```

Produces `target/release/geo_repair.{dll,so,dylib}` (shared) and
`libgeo_repair.a` (static) plus `libgeo_repair.rlib` (Rust). Ship
`include/geo_repair.h` with the binary artifact.

```c
#include "geo_repair.h"
```

Link against the shared library (or the static archive) and call
`geo_repair_version()` to verify the ABI.

### Quickstart

```c
#include <stdio.h>
#include "geo_repair.h"

int main(void) {
    /* WKB of a self-intersecting polygon */
    uint8_t bowtie[] = { /* ... */ };
    GeoRepairResult r = geo_repair_validate_and_fix(bowtie, sizeof(bowtie));
    if (r.success) {
        /* r.wkb_data / r.wkb_len hold the fixed geometry;
         * r.error_code == GeoRepairErrorCode_InvalidGeometry when the
         * input was repaired, with the reasons in r.error_msg */
    }
    geo_repair_free_result(&r);
    return 0;
}
```

### Error model

Every result carries a `GeoRepairErrorCode`:

| Code | Meaning |
|---|---|
| `None` | Operation succeeded. |
| `Parse` | Input WKB/WKT could not be parsed. |
| `InvalidInput` | Null pointer or invalid length argument. |
| `InvalidGeometry` | Validation found violations (on validate / validate-and-fix paths, where an invalid geometry is the *result*). |
| `Encode` | Output could not be encoded (reserved; the current writers are infallible). |
| `Panic` | An internal panic was caught. Report as a bug. |

On `validate_and_fix*`, a *repaired* input returns `success == true` with
`error_code == InvalidGeometry` and the reasons in `error_msg`; a valid
input returns `error_code == None` and a null `error_msg`.

### API surface

WKB (all take `const uint8_t* wkb_data, size_t wkb_len`):

| Function | Notes |
|---|---|
| `geo_repair_make_valid` | Default config (Auto). |
| `geo_repair_make_valid_with_config` | `keep_collapsed`, `poly_method` (0 Auto, 1 Arrange, 2 Structure). |
| `geo_repair_make_valid_with_config_full` | Adds `fill_rule` (0 EvenOdd, 1 NonZero) and `epsg_code` (<= 0 unknown). |
| `geo_repair_is_valid` | Returns 1/0; 0 on parse failure. |
| `geo_repair_validate` | `success` = validity; reasons in `error_msg`. |
| `geo_repair_validate_reason` | Alias of `geo_repair_validate`. |
| `geo_repair_validate_and_fix` | Fix + reasons when repaired. |
| `geo_repair_validate_and_fix_with_config` | Config variant. |
| `geo_repair_make_valid_batch` | `(const GeoRepairWkbBuffer* inputs, size_t count, int parallel)`; parallel != 0 uses the rayon batch when built with the `parallel` feature. Per-item parse failures are per-item results; the batch itself succeeds. |

WKT (all take `const char* wkt`), returning `GeoRepairStringResult`:
`geo_repair_make_valid_wkt`, `..._with_config`, `..._with_config_full`,
`geo_repair_is_valid_wkt`, `geo_repair_validate_wkt`,
`geo_repair_validate_and_fix_wkt`, `..._with_config`.

Memory: `geo_repair_free_result`, `geo_repair_free_string_result`,
`geo_repair_free_batch_result`. Each zeroes the struct; double-free and
null are no-ops.

### ABI stability

The struct layouts and `GeoRepairErrorCode` values are fixed from
0.14.2. Adding codes or functions is additive; renumbering or removing
is a breaking ABI change. The panic-safety guarantee requires the
library to be built with `panic = "unwind"` (the shipped release profile
uses unwind; building with `panic = "abort"` disables containment).

### Releasing artifacts

The `release.yml` workflow builds the C libraries for Windows (x64),
Linux (x64), and macOS (x64 + arm64), packages each platform's shared +
static libs with the header, and attaches them to the GitHub release.

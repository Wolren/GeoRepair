# Integration with the geo ecosystem

geo-repair is built on `geo` types and plugs into the georust ecosystem
two ways.

## geo-traits sources

Feature flag: `geo-traits` (not default).

The `interop` module runs validation and repair over
`geo_traits::GeometryTrait` / `geo_traits::GeometryCollectionTrait`, the
trait layer implemented by `geo`, geoarrow, geozero, and `wkb`. Any such
source can be validated or repaired in one call
(`interop::is_valid_geometry`, `interop::validate_geometry`,
`interop::make_valid_geometry`, `interop::make_valid_geometries`,
`interop::make_valid_geometry_collection`) without the caller
materializing geo types; repair results come back as `geo::Geometry<f64>`,
and callers that need their own representation back convert with their
crate's geo-traits writer (e.g. `wkb::writer::write_wkb`, geoarrow's
`ToWKB`).

```rust
# use geo::{Geometry, Point};
# let geometry = Geometry::Point(Point::new(0.0, 0.0));
#[cfg(feature = "geo-traits")]
{
    use geo_repair::interop::{is_valid_geometry, make_valid_geometry};

    // `geometry` is any geo-traits source; here a geo type.
    let ok = is_valid_geometry(&geometry);
    let fixed = make_valid_geometry(&geometry);
}
```

Validation-only entry points: `is_valid_geometry` (quick boolean) and
`validate_geometry` (full OGC check, returns `ValidationResult`).

## geo's `Validation` trait

Always available, no feature flag.

`GeoRepairValidation(&geometry)` wraps any `&geo::Geometry<f64>` and
exposes geo_repair's validator through geo's `Validation` trait
(`.is_valid()`, `.check_validation()`, `.validation_errors()`) with geo's
`Invalid*` error taxonomy. The orphan rule prevents implementing geo's
trait for geo's own types, so the adapter is the bridge.

Mapping is best-effort: geo_repair's stricter gates (32-ulp collinear,
T-junction) surface through it, and classes geo does not model (ring
closure, orientation, duplicates, ...) are omitted from the geo view.
Those stay visible through the plain `geo_repair::validate` API.

```rust
use geo::algorithm::validation::Validation;
use geo_repair::GeoRepairValidation;

let adapter = GeoRepairValidation(&geometry);
assert!(!adapter.is_valid());
```

## See also

- `docs/ARCHITECTURE.md` section 3: the validation model the adapter exposes.
- Feature list: `README.md` (`geo-traits` row).

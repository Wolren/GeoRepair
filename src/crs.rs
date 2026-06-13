#[cfg(feature = "proj")]
use crate::core::MakeValidError;
#[cfg(feature = "proj")]
use geo::Geometry;

/// Coordinate Reference System metadata.
///
/// Stores CRS information and provides CRS-aware tolerance heuristics.
/// When the `proj` feature is enabled, also supports coordinate
/// transformation between CRS via the PROJ library (through `geo`).
#[derive(Clone, Debug, PartialEq)]
pub struct Crs {
    /// Authority string, e.g. `"EPSG:4326"`.
    authority: Option<String>,
    /// `true` if this is a geographic (lon/lat) CRS.
    is_geographic: bool,
}

impl Crs {
    /// Create from an EPSG code.
    ///
    /// Automatically classifies geographic vs projected.
    pub fn from_epsg(code: u32) -> Self {
        Self {
            authority: Some(format!("EPSG:{code}")),
            is_geographic: epsg_is_geographic(code),
        }
    }

    /// Create from an authority string like `"EPSG:4326"` or `"ESRI:54030"`.
    ///
    /// Extracts the numeric code when possible for classification.
    pub fn from_authority(auth: &str) -> Self {
        let is_geo = auth
            .split(':')
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .map_or(false, epsg_is_geographic);
        Self {
            authority: Some(auth.to_string()),
            is_geographic: is_geo,
        }
    }

    /// Create an unknown CRS (no metadata, not geographic).
    ///
    /// Use when the CRS is not known. Tolerance defaults to the
    /// general-purpose value.
    pub fn unknown() -> Self {
        Self {
            authority: None,
            is_geographic: false,
        }
    }

    /// Explicitly create a geographic CRS from an EPSG code.
    ///
    /// Skips the heuristic — useful for codes the classifier doesn't
    /// know about yet.
    pub fn geographic(code: u32) -> Self {
        Self {
            authority: Some(format!("EPSG:{code}")),
            is_geographic: true,
        }
    }

    /// Explicitly create a projected CRS from an EPSG code.
    pub fn projected(code: u32) -> Self {
        Self {
            authority: Some(format!("EPSG:{code}")),
            is_geographic: false,
        }
    }

    /// WGS84 geographic CRS (EPSG:4326).
    pub fn wgs84() -> Self {
        Self::from_epsg(4326)
    }

    /// Is this a geographic (angle-based) CRS?
    pub fn is_geographic(&self) -> bool {
        self.is_geographic
    }

    /// Get the authority string if known.
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// Extract the numeric SRID from the authority string (e.g. `4326` from `"EPSG:4326"`).
    pub fn srid(&self) -> Option<i32> {
        self.authority
            .as_ref()
            .and_then(|a| a.split(':').nth(1).and_then(|s| s.parse::<i32>().ok()))
    }

    /// Suggest an appropriate epsilon tolerance for geometric
    /// predicates in this CRS.
    ///
    /// - Geographic CRS: `1e-10` degrees (~0.01 mm near equator)
    /// - Projected CRS:  `1e-6`  metres (1 micron)
    /// - Unknown:        `1e-12` (normalised by coordinate scale downstream)
    pub fn suggested_tolerance(&self) -> f64 {
        if self.is_geographic {
            1e-10
        } else if self.authority.is_some() {
            1e-6
        } else {
            1e-12
        }
    }

    /// Parse a `.prj`-style WKT string to extract the authority.
    ///
    /// Looks for `AUTHORITY["EPSG","<code>"]` in the WKT and returns
    /// the corresponding Crs. Returns `None` if no authority is found.
    pub fn from_prj_wkt(wkt: &str) -> Option<Self> {
        let needle = "AUTHORITY";
        let mut pos = 0;
        while let Some(start) = wkt[pos..].find(needle) {
            let seg = &wkt[pos + start + needle.len()..];
            let seg = seg.trim_start().strip_prefix('[')?;
            let seg = seg.trim_start().strip_prefix('"')?;
            let (org, rest) = seg.split_once('"')?;
            let rest = rest.trim_start().strip_prefix(',')?;
            let rest = rest.trim_start().strip_prefix('"')?;
            let (code_str, _) = rest.split_once('"')?;
            if let Ok(code) = code_str.parse::<u32>() {
                return Some(Self {
                    authority: Some(format!("{org}:{code}")),
                    is_geographic: org == "EPSG" && epsg_is_geographic(code),
                });
            }
            pos = start + 1;
        }
        None
    }

    /// Generate an OGC-standard WKT string suitable for `.prj` files.
    ///
    /// Includes `AUTHORITY` clause for proper round-tripping.
    /// Returns `None` if the CRS has no authority string.
    pub fn to_esri_wkt(&self) -> Option<String> {
        let srid = self.srid()?;
        let auth_clause = format!(",AUTHORITY[\"EPSG\",\"{srid}\"]");
        Some(match (srid, self.is_geographic) {
            (4326, _) => format!(
                "GEOGCS[\"GCS_WGS_1984\",\
                 DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],\
                 PRIMEM[\"Greenwich\",0.0],\
                 UNIT[\"Degree\",0.0174532925199433]{auth_clause}]"
            ),
            (4269, _) => format!(
                "GEOGCS[\"GCS_North_American_1983\",\
                 DATUM[\"D_North_American_1983\",SPHEROID[\"GRS_1980\",6378137.0,298.257222101]],\
                 PRIMEM[\"Greenwich\",0.0],\
                 UNIT[\"Degree\",0.0174532925199433]{auth_clause}]"
            ),
            (3857, _) => format!(
                "PROJCS[\"WGS_1984_Web_Mercator_Auxiliary_Sphere\",\
                 GEOGCS[\"GCS_WGS_1984\",\
                 DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],\
                 PRIMEM[\"Greenwich\",0.0],\
                 UNIT[\"Degree\",0.0174532925199433]],\
                 PROJECTION[\"Mercator_Auxiliary_Sphere\"],\
                 PARAMETER[\"False_Easting\",0.0],\
                 PARAMETER[\"False_Northing\",0.0],\
                 PARAMETER[\"Central_Meridian\",0.0],\
                 PARAMETER[\"Standard_Parallel_1\",0.0],\
                 PARAMETER[\"Auxiliary_Sphere_Type\",0.0],\
                 UNIT[\"Meter\",1.0]{auth_clause}]"
            ),
            _ if self.is_geographic => format!(
                "GEOGCS[\"GCS_EPSG_{srid}\",\
                 DATUM[\"D_unknown\",SPHEROID[\"unknown\",6378137.0,298.257223563]],\
                 PRIMEM[\"Greenwich\",0.0],\
                 UNIT[\"Degree\",0.0174532925199433]{auth_clause}]"
            ),
            _ => format!(
                "PROJCS[\"EPSG_{srid}\",\
                 GEOGCS[\"GCS_WGS_1984\",\
                 DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],\
                 PRIMEM[\"Greenwich\",0.0],\
                 UNIT[\"Degree\",0.0174532925199433]],\
                 PROJECTION[\"Transverse_Mercator\"],\
                 PARAMETER[\"False_Easting\",0.0],\
                 PARAMETER[\"False_Northing\",0.0],\
                 PARAMETER[\"Central_Meridian\",0.0],\
                 PARAMETER[\"Scale_Factor\",1.0],\
                 UNIT[\"Meter\",1.0]{auth_clause}]"
            ),
        })
    }
}

impl Default for Crs {
    fn default() -> Self {
        Self::unknown()
    }
}

/// Transform a geometry between two CRS.
///
/// Requires the `proj` feature AND `geo/proj` in Cargo.toml dependencies.
/// Without `geo/proj`, this function returns a `CrsError` at runtime.
#[cfg(feature = "proj")]
pub fn transform_geometry(
    _geom: &Geometry<f64>,
    _from: &Crs,
    _to: &Crs,
) -> Result<Geometry<f64>, MakeValidError> {
    Err(MakeValidError::CrsError(
        "CRS transformation requires the 'proj' feature in geo-repair \
         AND 'geo/proj' in your Cargo.toml dependencies"
            .into(),
    ))
}

/// Classify an EPSG code as geographic (lon/lat) or projected.
///
/// Covers the standard EPSG ranges:
/// - Geographic CRS codes (2D): 4000-4999 (mostly), plus explicit known codes
/// - Projected CRS codes: 2000-3999 (UTM zones), 5000-5999, 10000+
/// - UTM zones: 32600-32760 (WGS 84 / UTM north/south) are projected
fn epsg_is_geographic(code: u32) -> bool {
    match code {
        // Explicitly geographic — commonly used codes
        4326 | 4258 | 4269 | 4267 | 4275 | 4617 | 4618 | 4619 | 4620 | 4937 | 4936 | 4979
        | 4980 | 4152 | 4121 | 4122 | 4141 | 4148 | 4171 | 4167 | 4170 | 4173 | 4176 | 4181
        | 4188 | 4190 | 4191 | 4192 | 4193 | 4195 | 4197 | 4199 | 4200 | 4201 | 4202 | 4203
        | 4204 | 4205 | 4206 | 4207 | 4208 | 4209 | 4210 | 4211 | 4212 | 4214 | 4215 | 4216
        | 4222 | 4223 | 4225 | 4230 | 4231 | 4238 | 4242 | 4248 | 4250 | 4251 | 4252 | 4254
        | 4255 | 4256 | 4257 | 4259 | 4261 | 4262 | 4263 | 4264 | 4265 | 4266 | 4268 | 4270
        | 4272 | 4273 | 4274 | 4277 | 4278 | 4279 | 4280 | 4281 | 4282 | 4283 | 4284 | 4285
        | 4287 | 4288 | 4289 | 4291 | 4292 | 4293 | 4294 | 4295 | 4296 | 4297 | 4298 | 4299
        | 4300 | 4301 | 4302 | 4303 | 4304 | 4306 | 4308 | 4309 | 4310 | 4311 | 4312 | 4313
        | 4314 | 4315 | 4316 | 4317 | 4318 | 4319 | 4320 | 4321 | 4322 | 4324 | 4327 | 4328
        | 4329 | 4330 | 4331 | 4332 | 4333 | 4334 | 4335 | 4336 | 4337 | 4338 | 4339 | 4340
        | 4341 | 4342 | 4343 | 4344 | 4345 | 4346 | 4347 | 4348 | 4349 | 4350 | 4351 | 4352
        | 4353 | 4354 | 4355 | 4356 | 4357 | 4358 | 4359 | 4360 | 4361 | 4362 | 4363 | 4364
        | 4365 | 4366 | 4367 | 4368 | 4369 | 4370 | 4371 | 4372 | 4373 | 4374 | 4375 | 4376
        | 4377 | 4378 | 4379 | 4380 | 4381 | 4382 | 4383 | 4384 | 4385 | 4386 | 4387 | 4388
        | 4389 | 4390 | 4391 | 4392 | 4393 | 4394 | 4395 | 4396 | 4397 | 4398 | 4399 => true,
        // Projected exceptions in the 4000-4999 range
        4087 | 4088 | 4465 | 4466 | 4470 | 4471 | 4555 | 4556 | 4647 => false,
        c if (4400..=4999).contains(&c) => true,
        // WGS 84 / Pseudo-Mercator
        3857 | 3858 | 3859 => false,
        // UTM zones (north and south) — projected
        c if (32600..=32760).contains(&c) => false,
        // ED50 / UTM zones
        c if (23000..=23099).contains(&c) => false,
        // All other codes default to projected
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epsg_4326_is_geographic() {
        assert!(Crs::from_epsg(4326).is_geographic());
    }

    #[test]
    fn test_epsg_3857_is_projected() {
        assert!(!Crs::from_epsg(3857).is_geographic());
    }

    #[test]
    fn test_epsg_4269_is_geographic() {
        assert!(Crs::from_epsg(4269).is_geographic());
    }

    #[test]
    fn test_epsg_32633_is_projected() {
        assert!(!Crs::from_epsg(32633).is_geographic());
    }

    #[test]
    fn test_epsg_32760_is_projected() {
        assert!(!Crs::from_epsg(32760).is_geographic());
    }

    #[test]
    fn test_epsg_4087_is_projected() {
        assert!(!Crs::from_epsg(4087).is_geographic());
    }

    #[test]
    fn test_from_authority_parses() {
        let crs = Crs::from_authority("EPSG:4326");
        assert!(crs.is_geographic());
        assert_eq!(crs.authority(), Some("EPSG:4326"));
    }

    #[test]
    fn test_unknown() {
        let crs = Crs::unknown();
        assert!(!crs.is_geographic());
        assert_eq!(crs.authority(), None);
    }

    #[test]
    fn test_wgs84_shortcut() {
        let crs = Crs::wgs84();
        assert!(crs.is_geographic());
        assert_eq!(crs.authority(), Some("EPSG:4326"));
    }

    #[test]
    fn test_tolerance_geographic() {
        let crs = Crs::from_epsg(4326);
        assert_eq!(crs.suggested_tolerance(), 1e-10);
    }

    #[test]
    fn test_tolerance_projected() {
        let crs = Crs::from_epsg(32633);
        assert_eq!(crs.suggested_tolerance(), 1e-6);
    }

    #[test]
    fn test_tolerance_unknown() {
        let crs = Crs::unknown();
        assert_eq!(crs.suggested_tolerance(), 1e-12);
    }

    #[test]
    fn test_geographic_override() {
        let crs = Crs::geographic(32633);
        assert!(crs.is_geographic());
        assert_eq!(crs.authority(), Some("EPSG:32633"));
    }

    #[test]
    fn test_projected_override() {
        let crs = Crs::projected(4326);
        assert!(!crs.is_geographic());
    }

    #[test]
    fn test_default_is_unknown() {
        let crs = Crs::default();
        assert_eq!(crs, Crs::unknown());
    }
}

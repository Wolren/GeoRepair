
//! Stores crs information (authority, geographic or projected) and tolerance heuristics
//! per crs type. The Crs type is embedded in crate::Feature for I/O round-tripping and
//! in crate::MakeValidConfig for crs-aware tolerance adjustment. # crs text (WKT)
//! crs::to_wkt2 emits a WKT2:2019 definition (iso 19162:2019, OGC 18-010r11) for the
//! EPSG CRSs bundled with the crate. crs::from_wkt extracts the authority from WKT1 or
//! WKT2 text. crs::to_esri_wkt produces esri-flavoured WKT1 for .prj consumers.

#[cfg(feature = "proj")]
use crate::core::MakeValidError;
use alloc::string::String;
use alloc::string::ToString;
#[cfg(feature = "proj")]
use geo::Geometry;

/// Stores crs information and provides crs-aware tolerance heuristics.
#[derive(Clone, Debug, PartialEq)]
pub struct Crs {
    /// Authority string, e.g.
    authority: Option<String>,
    /// `true` if this is a geographic (lon/lat) crs.
    is_geographic: bool,
}

impl Crs {
    /// Create from an EPSG code.
    pub fn from_epsg(code: u32) -> Self {
        Self {
            authority: Some(format!("EPSG:{code}")),
            is_geographic: epsg_is_geographic(code),
        }
    }

    /// Create from an authority string like `"EPSG:4326"` or `"esri:54030"`.
    pub fn from_authority(auth: &str) -> Self {
        let is_geo = auth
            .split(':')
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .is_some_and(epsg_is_geographic);
        Self {
            authority: Some(auth.to_string()),
            is_geographic: is_geo,
        }
    }

    /// Tolerance defaults to the general-purpose value.
    pub fn unknown() -> Self {
        Self {
            authority: None,
            is_geographic: false,
        }
    }

    /// Explicitly create a geographic crs from an EPSG code.
    pub fn geographic(code: u32) -> Self {
        Self {
            authority: Some(format!("EPSG:{code}")),
            is_geographic: true,
        }
    }

    /// Explicitly create a projected crs from an EPSG code.
    pub fn projected(code: u32) -> Self {
        Self {
            authority: Some(format!("EPSG:{code}")),
            is_geographic: false,
        }
    }

    /// WGS84 geographic crs (EPSG:4326).
    pub fn wgs84() -> Self {
        Self::from_epsg(4326)
    }

    /// Is this a geographic (angle-based) crs?
    pub fn is_geographic(&self) -> bool {
        self.is_geographic
    }

    /// Get the authority string if known.
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// Extract the numeric SRID from the authority string (e.g. `4326` from
    /// `"EPSG:4326"`).
    pub fn srid(&self) -> Option<i32> {
        self.authority
            .as_ref()
            .and_then(|a| a.split(':').nth(1).and_then(|s| s.parse::<i32>().ok()))
    }

    /// Suggest an appropriate epsilon tolerance for geometric predicates in this crs.
    ///
    /// - Geographic crs: `1e-10` degrees (~0.01 mm near equator)
    /// - Projected crs: `1e-6` metres (1 micron)
    /// - Unknown: `1e-12` (normalised by coordinate scale downstream)
    pub fn suggested_tolerance(&self) -> f64 {
        if self.is_geographic {
            1e-10
        } else if self.authority.is_some() {
            1e-6
        } else {
            1e-12
        }
    }

    /// Returns `None` when the string carries no identifier, which is common for
    /// hand-authored `.prj` files.
    ///
    /// # Example
    /// ```
    /// use geo_repair::Crs;
    ///
    /// let wkt = Crs::from_epsg(4326).to_wkt2().unwrap();
    /// assert_eq!(Crs::from_wkt(&wkt), Some(Crs::from_epsg(4326)));
    /// ```
    pub fn from_wkt(wkt: &str) -> Option<Self> {
        let (org, code) = last_identifier(wkt)?;
        Some(Self {
            authority: Some(format!("{org}:{code}")),
            is_geographic: org == "EPSG" && epsg_is_geographic(code),
        })
    }

    /// Returns `None` if no authority is found.
    pub fn from_prj_wkt(wkt: &str) -> Option<Self> {
        Self::from_wkt(wkt)
    }

    /// Those return `None` rather than a fabricated (and probably wrong) datum or
    /// projection.
    ///
    /// # Example
    /// ```
    /// use geo_repair::Crs;
    ///
    /// let wkt = Crs::from_epsg(3857).to_wkt2().unwrap();
    /// assert!(wkt.starts_with("PROJCRS["));
    /// assert!(Crs::from_epsg(32633).to_wkt2().is_none());
    /// ```
    pub fn to_wkt2(&self) -> Option<String> {
        match self.authority.as_deref()? {
            "EPSG:4326" => Some(String::from(WKT2_EPSG_4326)),
            "EPSG:4269" => Some(String::from(WKT2_EPSG_4269)),
            "EPSG:3857" => Some(String::from(WKT2_EPSG_3857)),
            _ => None,
        }
    }

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

const WKT2_EPSG_4326: &str = r#"GEOGCRS["WGS 84",
    ENSEMBLE["World Geodetic System 1984 ensemble",
        MEMBER["World Geodetic System 1984 (Transit)"],
        MEMBER["World Geodetic System 1984 (G730)"],
        MEMBER["World Geodetic System 1984 (G873)"],
        MEMBER["World Geodetic System 1984 (G1150)"],
        MEMBER["World Geodetic System 1984 (G1674)"],
        MEMBER["World Geodetic System 1984 (G1762)"],
        MEMBER["World Geodetic System 1984 (G2139)"],
        MEMBER["World Geodetic System 1984 (G2296)"],
        ELLIPSOID["WGS 84",6378137,298.257223563,
            LENGTHUNIT["metre",1]],
        ENSEMBLEACCURACY[2.0]],
    PRIMEM["Greenwich",0,
        ANGLEUNIT["degree",0.0174532925199433]],
    CS[ellipsoidal,2],
    AXIS["geodetic latitude (Lat)",north,
        ORDER[1],
        ANGLEUNIT["degree",0.0174532925199433]],
    AXIS["geodetic longitude (Lon)",east,
        ORDER[2],
        ANGLEUNIT["degree",0.0174532925199433]],
    USAGE[
        SCOPE["Horizontal component of 3D system."],
        AREA["World."],
        BBOX[-90,-180,90,180]],
    ID["EPSG",4326]]"#;

/// Authoritative WKT2:2019 definition of EPSG:4269 (NAD83).
const WKT2_EPSG_4269: &str = r#"GEOGCRS["NAD83",
    DATUM["North American Datum 1983",
        ELLIPSOID["GRS 1980",6378137,298.257222101,
            LENGTHUNIT["metre",1]]],
    PRIMEM["Greenwich",0,
        ANGLEUNIT["degree",0.0174532925199433]],
    CS[ellipsoidal,2],
    AXIS["geodetic latitude (Lat)",north,
        ORDER[1],
        ANGLEUNIT["degree",0.0174532925199433]],
    AXIS["geodetic longitude (Lon)",east,
        ORDER[2],
        ANGLEUNIT["degree",0.0174532925199433]],
    USAGE[
        SCOPE["Geodesy."],
        AREA["North America - onshore and offshore: Canada. Puerto Rico. United States (USA). US Virgin Islands. British Virgin Islands."],
        BBOX[14.92,167.65,86.46,-40.73]],
    ID["EPSG",4269],
    REMARK["Longitude is POSITIVE EAST. The adjustment included connections to Greenland and Mexico but the system was not adopted there. For applications with an accuracy of better than 1m replaced by NAD83(HARN) in the US and PRVI and by NAD83(CSRS) in Canada."]]"#;

/// Authoritative WKT2:2019 definition of EPSG:3857 (wgs 84 / Pseudo-Mercator).
const WKT2_EPSG_3857: &str = r#"PROJCRS["WGS 84 / Pseudo-Mercator",
    BASEGEOGCRS["WGS 84",
        ENSEMBLE["World Geodetic System 1984 ensemble",
            MEMBER["World Geodetic System 1984 (Transit)"],
            MEMBER["World Geodetic System 1984 (G730)"],
            MEMBER["World Geodetic System 1984 (G873)"],
            MEMBER["World Geodetic System 1984 (G1150)"],
            MEMBER["World Geodetic System 1984 (G1674)"],
            MEMBER["World Geodetic System 1984 (G1762)"],
            MEMBER["World Geodetic System 1984 (G2139)"],
            MEMBER["World Geodetic System 1984 (G2296)"],
            ELLIPSOID["WGS 84",6378137,298.257223563,
                LENGTHUNIT["metre",1]],
            ENSEMBLEACCURACY[2.0]],
        PRIMEM["Greenwich",0,
            ANGLEUNIT["degree",0.0174532925199433]],
        ID["EPSG",4326]],
    CONVERSION["Popular Visualisation Pseudo-Mercator",
        METHOD["Popular Visualisation Pseudo Mercator",
            ID["EPSG",1024]],
        PARAMETER["Latitude of natural origin",0,
            ANGLEUNIT["degree",0.0174532925199433],
            ID["EPSG",8801]],
        PARAMETER["Longitude of natural origin",0,
            ANGLEUNIT["degree",0.0174532925199433],
            ID["EPSG",8802]],
        PARAMETER["False easting",0,
            LENGTHUNIT["metre",1],
            ID["EPSG",8806]],
        PARAMETER["False northing",0,
            LENGTHUNIT["metre",1],
            ID["EPSG",8807]]],
    CS[Cartesian,2],
    AXIS["easting (X)",east,
        ORDER[1],
        LENGTHUNIT["metre",1]],
    AXIS["northing (Y)",north,
        ORDER[2],
        LENGTHUNIT["metre",1]],
    USAGE[
        SCOPE["Web mapping and visualisation."],
        AREA["World between 85.06°S and 85.06°N."],
        BBOX[-85.06,-180,85.06,180]],
    ID["EPSG",3857]]"#;

#[inline]
fn skip_ws(s: &[u8], i: &mut usize) {
    while *i < s.len() && s[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

#[inline]
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn last_identifier(wkt: &str) -> Option<(&str, u32)> {
    let s = wkt.as_bytes();
    let n = s.len();
    let mut i = 0;
    let mut result = None;
    while i < n {
        let b = s[i];
        if b == b'"' {
            i += 1;
            while i < n && s[i] != b'"' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if !is_ident_byte(b) {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < n && is_ident_byte(s[i]) {
            i += 1;
        }
        let word = &wkt[start..i];
        if (word == "ID" || word == "AUTHORITY")
            && let Some((org, code)) = parse_clause(wkt, i)
        {
            result = Some((org, code));
        }
    }
    result
}

fn parse_clause(wkt: &str, mut i: usize) -> Option<(&str, u32)> {
    let s = wkt.as_bytes();
    let n = s.len();
    skip_ws(s, &mut i);
    if i >= n || s[i] != b'[' {
        return None;
    }
    i += 1;
    skip_ws(s, &mut i);
    if i >= n || s[i] != b'"' {
        return None;
    }
    i += 1;
    let org_start = i;
    while i < n && s[i] != b'"' {
        i += 1;
    }
    if i >= n {
        return None;
    }
    let org = &wkt[org_start..i];
    i += 1; // closing quote of the organisation
    skip_ws(s, &mut i);
    if i >= n || s[i] != b',' {
        return None;
    }
    i += 1;
    skip_ws(s, &mut i);
    if i < n && s[i] == b'"' {
        i += 1;
        skip_ws(s, &mut i);
    }
    let code_start = i;
    while i < n && s[i].is_ascii_digit() {
        i += 1;
    }
    if i == code_start {
        return None;
    }
    let code = wkt[code_start..i].parse::<u32>().ok()?;
    Some((org, code))
}

impl Default for Crs {
    fn default() -> Self {
        Self::unknown()
    }
}

#[cfg(feature = "proj")]
pub fn transform_geometry(
    geom: &Geometry<f64>,
    from: &Crs,
    to: &Crs,
) -> Result<Geometry<f64>, MakeValidError> {
    use geo::algorithm::Transform;

    let from_auth = from
        .authority()
        .ok_or_else(|| MakeValidError::CrsError("source CRS has no authority".into()))?;
    let to_auth = to
        .authority()
        .ok_or_else(|| MakeValidError::CrsError("target CRS has no authority".into()))?;

    let proj = geo::algorithm::proj::Proj::new_known_crs(from_auth, to_auth, None)
        .map_err(|e| MakeValidError::CrsError(format!("PROJ init: {e}")))?;

    geom.transformed(&proj)
        .map_err(|e| MakeValidError::CrsError(format!("PROJ transform: {e}")))
}

fn epsg_is_geographic(code: u32) -> bool {
    match code {
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
        4087 | 4088 | 4465 | 4466 | 4470 | 4471 | 4555 | 4556 | 4647 => false,
        c if (4400..=4999).contains(&c) => true,
        3857..=3859 => false,
        c if (32600..=32760).contains(&c) => false,
        c if (23000..=23099).contains(&c) => false,
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

    #[test]
    fn test_to_wkt2_known_codes() {
        assert!(
            Crs::from_epsg(4326)
                .to_wkt2()
                .unwrap()
                .starts_with("GEOGCRS[")
        );
        assert!(
            Crs::from_epsg(4269)
                .to_wkt2()
                .unwrap()
                .starts_with("GEOGCRS[")
        );
        assert!(
            Crs::from_epsg(3857)
                .to_wkt2()
                .unwrap()
                .starts_with("PROJCRS[")
        );
    }

    #[test]
    fn test_to_wkt2_is_wkt2_not_wkt1() {
        let w = Crs::from_epsg(4326).to_wkt2().unwrap();
        assert!(w.contains("GEOGCRS["));
        assert!(w.contains("ID[\"EPSG\",4326]"));
        assert!(w.contains("ENSEMBLE["));
        assert!(!w.contains("GEOGCS["));
        assert!(!w.contains("AUTHORITY["));
    }

    #[test]
    fn test_to_wkt2_none_for_unbundled_code() {
        assert_eq!(Crs::from_epsg(32633).to_wkt2(), None);
        assert_eq!(Crs::unknown().to_wkt2(), None);
        assert_eq!(Crs::from_authority("ESRI:4326").to_wkt2(), None);
    }

    #[test]
    fn test_to_wkt2_roundtrip() {
        for code in [4326u32, 4269, 3857] {
            let wkt = Crs::from_epsg(code).to_wkt2().unwrap();
            assert_eq!(
                Crs::from_wkt(&wkt),
                Some(Crs::from_epsg(code)),
                "EPSG:{code}"
            );
        }
    }

    #[test]
    fn test_from_wkt_wkt2_id() {
        assert_eq!(
            Crs::from_wkt(r#"GEOGCRS["WGS 84",ID["EPSG",4326]]"#),
            Some(Crs::from_epsg(4326))
        );
    }

    #[test]
    fn test_from_wkt_wkt1_authority() {
        assert_eq!(
            Crs::from_wkt(r#"GEOGCS["WGS 84",AUTHORITY["EPSG","4326"]]"#),
            Some(Crs::from_epsg(4326))
        );
    }

    #[test]
    fn test_from_wkt_prefers_crs_level_identifier() {
        let wkt = Crs::from_epsg(3857).to_wkt2().unwrap();
        assert_eq!(Crs::from_wkt(&wkt).unwrap().srid(), Some(3857));
    }

    #[test]
    fn test_from_wkt_ignores_quoted_and_substring_id() {
        let wkt = r#"GEOGCRS["ID test",REMARK["see ID["ESRI",9999]"],ID["ESRI",102100]]"#;
        assert_eq!(Crs::from_wkt(wkt), Some(Crs::from_authority("ESRI:102100")));
    }

    #[test]
    fn test_from_wkt_none_without_identifier() {
        assert_eq!(Crs::from_wkt(r#"GEOGCRS["No identifier here"]"#), None);
        let esri = r#"PROJCS["Albers Equal Area",GEOGCS["clark66",DATUM["D_North_American_1927",SPHEROID["Clarke_1866",6378206.4,294.9786982]]]]"#;
        assert_eq!(Crs::from_wkt(esri), None);
    }

    #[test]
    fn test_from_prj_wkt_is_alias_of_from_wkt() {
        let wkt = r#"GEOGCS["WGS 84",AUTHORITY["EPSG","4326"]]"#;
        assert_eq!(Crs::from_prj_wkt(wkt), Crs::from_wkt(wkt));
    }

    #[test]
    fn test_from_wkt_handles_wkt2_authority_alias() {
        assert_eq!(
            Crs::from_wkt(r#"GEOGCRS["NAD83",AUTHORITY["EPSG","4269"]]"#),
            Some(Crs::from_epsg(4269))
        );
    }
}

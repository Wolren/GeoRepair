use std::path::Path;

use crate::core::{MakeValidConfig, MakeValidError};
use crate::feature::Feature;

#[cfg(any(
    feature = "io-geojson",
    feature = "io-wkt",
    feature = "io-csv",
    feature = "load-shp"
))]
use crate::core::io_err;
#[cfg(any(
    feature = "io-geojson",
    feature = "io-wkt",
    feature = "io-csv",
    feature = "load-shp"
))]
use geo::Geometry;
#[cfg(any(
    feature = "io-geojson",
    feature = "io-wkt",
    feature = "io-csv",
    feature = "load-shp"
))]
use std::fs::File;
#[cfg(any(
    feature = "io-geojson",
    feature = "io-wkt",
    feature = "io-csv",
    feature = "load-shp"
))]
use std::io::{BufRead, BufReader, BufWriter, Write};
#[cfg(feature = "io-geojson")]
use std::io::{Read, Seek, SeekFrom};
#[cfg(any(feature = "io-wkt", feature = "io-csv"))]
use std::str::FromStr;
#[cfg(any(feature = "io-wkt", feature = "io-csv"))]
use wkt::{ToWkt, Wkt};

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Streaming reader that yields features one at a time.
///
/// Memory is O(1) per feature — the caller processes and discards each
/// feature before the next is read.
pub trait FeatureReader {
    fn next(&mut self) -> Option<Result<Feature, MakeValidError>>;
    fn progress(&self) -> f64;
}

/// Streaming writer that accepts features one at a time.
///
/// The caller must call [`finish`](FeatureWriter::finish) after the last
/// [`write`](FeatureWriter::write) to flush buffers and close any format
/// delimiters.
pub trait FeatureWriter {
    fn write(&mut self, feature: Feature) -> Result<(), MakeValidError>;
    fn finish(&mut self) -> Result<(), MakeValidError>;
}

// ---------------------------------------------------------------------------
// WKT backend — one line per geometry
// ---------------------------------------------------------------------------

#[cfg(any(feature = "io-wkt", feature = "io-csv"))]
pub struct WktReader {
    lines: std::io::Lines<BufReader<File>>,
}

#[cfg(any(feature = "io-wkt", feature = "io-csv"))]
impl WktReader {
    pub fn open(path: &str) -> Result<Self, MakeValidError> {
        let file = File::open(path).map_err(|e| io_err(e.to_string()))?;
        let reader = BufReader::new(file);
        Ok(Self {
            lines: reader.lines(),
        })
    }
}

#[cfg(any(feature = "io-wkt", feature = "io-csv"))]
impl FeatureReader for WktReader {
    fn next(&mut self) -> Option<Result<Feature, MakeValidError>> {
        let line = self.lines.next()?;
        let line = match line {
            Ok(l) => l,
            Err(e) => return Some(Err(io_err(e.to_string()))),
        };
        if line.trim().is_empty() {
            return self.next();
        }

        let wkt = match Wkt::from_str(&line) {
            Ok(w) => w,
            Err(e) => return Some(Err(MakeValidError::ParseError(format!("WKT error: {e}")))),
        };
        let geom: Geometry<f64> = match wkt.try_into() {
            Ok(g) => g,
            Err(e) => return Some(Err(MakeValidError::ParseError(format!("WKT error: {e}")))),
        };

        Some(Ok(Feature::new(geom)))
    }

    fn progress(&self) -> f64 {
        0.0
    }
}

#[cfg(feature = "io-wkt")]
pub struct WktWriter {
    writer: BufWriter<File>,
}

#[cfg(feature = "io-wkt")]
impl WktWriter {
    pub fn open(path: &str) -> Result<Self, MakeValidError> {
        let file = File::create(path).map_err(|e| io_err(e.to_string()))?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }
}

#[cfg(feature = "io-wkt")]
impl FeatureWriter for WktWriter {
    fn write(&mut self, feature: Feature) -> Result<(), MakeValidError> {
        let wkt = feature.geometry.to_wkt();
        writeln!(self.writer, "{wkt}").map_err(|e| io_err(e.to_string()))?;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), MakeValidError> {
        self.writer.flush().map_err(|e| io_err(e.to_string()))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CSV backend — WKT column
// ---------------------------------------------------------------------------

#[cfg(feature = "io-csv")]
pub struct CsvReader {
    lines: std::io::Lines<BufReader<File>>,
    header_skipped: bool,
}

#[cfg(feature = "io-csv")]
impl CsvReader {
    pub fn open(path: &str) -> Result<Self, MakeValidError> {
        let file = File::open(path).map_err(|e| io_err(e.to_string()))?;
        let reader = BufReader::new(file);
        Ok(Self {
            lines: reader.lines(),
            header_skipped: false,
        })
    }
}

#[cfg(feature = "io-csv")]
impl FeatureReader for CsvReader {
    fn next(&mut self) -> Option<Result<Feature, MakeValidError>> {
        if !self.header_skipped {
            let _ = self.lines.next()?; // skip header line
            self.header_skipped = true;
        }

        let line = self.lines.next()?;
        let line = match line {
            Ok(l) => l,
            Err(e) => return Some(Err(io_err(e.to_string()))),
        };
        if line.trim().is_empty() {
            return self.next();
        }

        // CSV WKT values may be quoted
        let trimmed = line.trim().trim_matches('"').trim();
        let wkt = match Wkt::from_str(trimmed) {
            Ok(w) => w,
            Err(e) => {
                return Some(Err(MakeValidError::ParseError(format!(
                    "CSV WKT error: {e}"
                ))))
            }
        };
        let geom: Geometry<f64> = match wkt.try_into() {
            Ok(g) => g,
            Err(e) => {
                return Some(Err(MakeValidError::ParseError(format!(
                    "CSV WKT error: {e}"
                ))))
            }
        };

        Some(Ok(Feature::new(geom)))
    }

    fn progress(&self) -> f64 {
        0.0
    }
}

#[cfg(feature = "io-csv")]
pub struct CsvWriter {
    writer: BufWriter<File>,
}

#[cfg(feature = "io-csv")]
impl CsvWriter {
    pub fn open(path: &str) -> Result<Self, MakeValidError> {
        let file = File::create(path).map_err(|e| io_err(e.to_string()))?;
        let mut writer = BufWriter::new(file);
        writeln!(writer, "geometry").map_err(|e| io_err(e.to_string()))?;
        Ok(Self { writer })
    }
}

#[cfg(feature = "io-csv")]
impl FeatureWriter for CsvWriter {
    fn write(&mut self, feature: Feature) -> Result<(), MakeValidError> {
        let wkt = feature.geometry.to_wkt();
        writeln!(self.writer, "\"{wkt}\"").map_err(|e| io_err(e.to_string()))?;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), MakeValidError> {
        self.writer.flush().map_err(|e| io_err(e.to_string()))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// GeoJSON backend — streaming via file seeking to features array
// ---------------------------------------------------------------------------

#[cfg(feature = "io-geojson")]
pub struct GeoJsonReader {
    iter: Box<dyn Iterator<Item = Result<geojson::Feature, serde_json::Error>>>,
    single: Option<Result<Feature, MakeValidError>>,
}

#[cfg(feature = "io-geojson")]
impl GeoJsonReader {
    pub fn open(path: &str) -> Result<Self, MakeValidError> {
        let mut file = File::open(path).map_err(|e| io_err(e.to_string()))?;

        // Scan the first 64 KB to find the features array offset,
        // then seek directly to `[` and stream from there.
        let mut header_buf = vec![0u8; 65536];
        let n = file
            .read(&mut header_buf)
            .map_err(|e| io_err(e.to_string()))?;
        header_buf.truncate(n);
        let header_str = std::str::from_utf8(&header_buf)
            .map_err(|_| MakeValidError::ParseError("invalid UTF-8 in GeoJSON".into()))?;

        if let Some(array_offset) = find_features_array(header_str) {
            file.seek(SeekFrom::Start(array_offset as u64))
                .map_err(|e| io_err(e.to_string()))?;
            let reader = BufReader::new(file);
            let iter: Box<dyn Iterator<Item = Result<geojson::Feature, serde_json::Error>>> =
                Box::new(serde_json::Deserializer::from_reader(reader).into_iter());
            Ok(Self { iter, single: None })
        } else {
            // Not a FeatureCollection — parse the whole thing.
            file.seek(SeekFrom::Start(0))
                .map_err(|e| io_err(e.to_string()))?;
            let reader = BufReader::new(file);
            let geojson: geojson::GeoJson = serde_json::from_reader(reader)
                .map_err(|e| MakeValidError::ParseError(e.to_string()))?;
            let feature = match geojson {
                geojson::GeoJson::Feature(mut f) => convert_geojson_feature(&mut f, None),
                geojson::GeoJson::Geometry(geom) => {
                    match crate::io::geojson::convert_geojson_zm(geom) {
                        Ok(zm) => Ok(Feature::with_all(zm.geometry, None, None, zm.zm)),
                        Err(e) => Err(e),
                    }
                }
                _ => Err(MakeValidError::ParseError(
                    "unexpected GeoJSON structure".into(),
                )),
            };
            let iter = Box::new(std::iter::empty());
            Ok(Self {
                iter,
                single: Some(feature),
            })
        }
    }
}

#[cfg(feature = "io-geojson")]
impl FeatureReader for GeoJsonReader {
    fn next(&mut self) -> Option<Result<Feature, MakeValidError>> {
        if let Some(r) = self.single.take() {
            return Some(r);
        }
        match self.iter.next()? {
            Ok(mut gj_feature) => Some(convert_geojson_feature(&mut gj_feature, None)),
            Err(e) => Some(Err(MakeValidError::ParseError(e.to_string()))),
        }
    }

    fn progress(&self) -> f64 {
        0.0
    }
}

#[cfg(feature = "io-geojson")]
fn convert_geojson_feature(
    f: &mut geojson::Feature,
    crs: Option<crate::Crs>,
) -> Result<Feature, MakeValidError> {
    if let Some(gj_geom) = f.geometry.take() {
        match crate::io::geojson::convert_geojson_zm(gj_geom) {
            Ok(zm) => Ok(Feature::with_all(
                zm.geometry,
                f.properties.take(),
                crs,
                zm.zm,
            )),
            Err(e) => Err(e),
        }
    } else {
        Ok(Feature::with_all(
            Geometry::GeometryCollection(geo::GeometryCollection(Vec::new())),
            f.properties.take(),
            crs,
            Vec::new(),
        ))
    }
}

#[cfg(feature = "io-geojson")]
fn find_features_array(header: &str) -> Option<usize> {
    let key_pos = header.find(r#""features""#)?;
    let after_key = &header[key_pos + 10..];
    let colon_pos = after_key.find(':')?;
    let after_colon = &after_key[colon_pos + 1..];
    after_colon
        .find('[')
        .map(|b| key_pos + 10 + colon_pos + 1 + b)
}

#[cfg(feature = "io-geojson")]
pub struct GeoJsonWriter {
    writer: BufWriter<File>,
    first: bool,
}

#[cfg(feature = "io-geojson")]
impl GeoJsonWriter {
    pub fn open(path: &str) -> Result<Self, MakeValidError> {
        let file = File::create(path).map_err(|e| io_err(e.to_string()))?;
        let mut writer = BufWriter::new(file);
        write!(writer, r#"{{"type":"FeatureCollection","features":["#)
            .map_err(|e| io_err(e.to_string()))?;
        Ok(Self {
            writer,
            first: true,
        })
    }
}

#[cfg(feature = "io-geojson")]
impl FeatureWriter for GeoJsonWriter {
    fn write(&mut self, feature: Feature) -> Result<(), MakeValidError> {
        if !self.first {
            write!(self.writer, ",").map_err(|e| io_err(e.to_string()))?;
        }
        self.first = false;

        let gj_geom = crate::io::geojson::geo_geom_to_geojson(feature.geometry);
        let gj_feature = geojson::Feature {
            geometry: Some(gj_geom),
            properties: feature.properties,
            id: None,
            bbox: None,
            foreign_members: None,
        };
        let json = serde_json::to_string(&gj_feature)
            .map_err(|e| MakeValidError::ParseError(e.to_string()))?;
        write!(self.writer, "{json}").map_err(|e| io_err(e.to_string()))?;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), MakeValidError> {
        write!(self.writer, "]}}").map_err(|e| io_err(e.to_string()))?;
        self.writer.flush().map_err(|e| io_err(e.to_string()))?;
        Ok(())
    }
}

/// Catch-all reader that just loads all features into memory (non-streaming).
/// Used for formats where we don't have a streaming backend yet (shp, gpkg, etc.).
pub struct BufferedReader {
    features: Vec<Feature>,
    index: usize,
}

impl BufferedReader {
    pub fn new(features: Vec<Feature>) -> Self {
        Self { features, index: 0 }
    }
}

impl FeatureReader for BufferedReader {
    fn next(&mut self) -> Option<Result<Feature, MakeValidError>> {
        if self.index >= self.features.len() {
            return None;
        }
        let f = self.features[self.index].clone();
        self.index += 1;
        Some(Ok(f))
    }

    fn progress(&self) -> f64 {
        if self.features.is_empty() {
            return 1.0;
        }
        self.index as f64 / self.features.len() as f64
    }
}

// ---------------------------------------------------------------------------
// Format dispatch
// ---------------------------------------------------------------------------

/// Open a streaming reader for the given file path.
///
/// Returns a format-appropriate [`FeatureReader`] based on the file extension.
/// WKT and CSV use true line-by-line streaming.  GeoJSON uses file-seeking to
/// stream the features array.  Other formats fall back to buffered in-memory
/// loading via [`crate::io::load_features`].
pub fn open_reader(path: &str) -> Result<Box<dyn FeatureReader>, MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        #[cfg(any(feature = "io-wkt", feature = "io-csv"))]
        "wkt" => Ok(Box::new(WktReader::open(path)?)),
        #[cfg(feature = "io-csv")]
        "csv" => Ok(Box::new(CsvReader::open(path)?)),
        #[cfg(feature = "io-geojson")]
        "geojson" | "json" => Ok(Box::new(GeoJsonReader::open(path)?)),
        _ => {
            // Fallback: load all into memory and iterate.
            let features = crate::io::load_features(path)?;
            Ok(Box::new(BufferedReader::new(features)))
        }
    }
}

/// Open a streaming writer for the given file path.
///
/// WKT and CSV write one line per feature.  GeoJSON writes a streaming
/// FeatureCollection.  Other formats fall back to the buffered export.
pub fn open_writer(path: &str) -> Result<Box<dyn FeatureWriter>, MakeValidError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        #[cfg(feature = "io-wkt")]
        "wkt" => Ok(Box::new(WktWriter::open(path)?)),
        #[cfg(feature = "io-csv")]
        "csv" => Ok(Box::new(CsvWriter::open(path)?)),
        #[cfg(feature = "io-geojson")]
        "geojson" | "json" => Ok(Box::new(GeoJsonWriter::open(path)?)),
        _ => {
            // Fallback: in-memory collector then export.
            Ok(Box::new(BufferedWriter::new(path)))
        }
    }
}

/// Fallback writer that collects features and writes them all at once.
pub struct BufferedWriter {
    path: String,
    features: Vec<Feature>,
}

impl BufferedWriter {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            features: Vec::new(),
        }
    }
}

impl FeatureWriter for BufferedWriter {
    fn write(&mut self, feature: Feature) -> Result<(), MakeValidError> {
        self.features.push(feature);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), MakeValidError> {
        let features = std::mem::take(&mut self.features);
        crate::io::export_features(&features, &self.path)
    }
}

// ---------------------------------------------------------------------------
// Stream repair pipeline
// ---------------------------------------------------------------------------

/// Read features from `input_path`, repair each geometry, and write the
/// results to `output_path`.
///
/// Uses streaming readers/writers where available (WKT, CSV, GeoJSON),
/// falling back to buffered I/O for other formats.
///
/// Geometry repair is done per-feature using [`MakeValid`] with the given
/// configuration.
pub fn stream_repair(
    input_path: &str,
    output_path: &str,
    config: &MakeValidConfig,
) -> Result<(), MakeValidError> {
    use crate::make_valid::MakeValid;

    let mut reader = open_reader(input_path)?;
    let mut writer = open_writer(output_path)?;

    while let Some(result) = reader.next() {
        let feature = result?;
        let repaired = feature.geometry.make_valid_with_config(config);
        let fixed = feature.with_repaired_geometry(repaired);
        writer.write(fixed)?;
    }
    writer.finish()
}

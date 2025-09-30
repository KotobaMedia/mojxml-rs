use anyhow::Result;
use geo_types::Polygon;
use geoparquet_batch_writer::{BatchConfig, GeoParquetBatchWriter, GeoParquetRowData};
use mojxml_parser::{FeatureProperties, ParsedXML, 筆界未定構成筆};
use std::path::{Path, PathBuf};

#[derive(Clone, GeoParquetRowData)]
struct OutputRow {
    #[geo(geometry)]
    geom: Polygon<f64>,

    地図名: String,
    市区町村コード: String,
    市区町村名: String,
    座標系: String,
    測地系判別: Option<String>,
    筆id: String,
    精度区分: Option<String>,
    大字コード: String,
    丁目コード: String,
    小字コード: String,
    予備コード: String,
    大字名: Option<String>,
    丁目名: Option<String>,
    小字名: Option<String>,
    予備名: Option<String>,
    地番: String,
    座標値種別: Option<String>,

    代表点緯度: f64,
    代表点経度: f64,

    筆界未定構成筆: Vec<筆界未定構成筆>,
}

pub struct GeoParquetWriter {
    internal_writer: GeoParquetBatchWriter<OutputRow>,
    output_path: PathBuf,
    has_features: bool,
}

impl crate::writer::shared::Writer for GeoParquetWriter {
    fn new(output_path: &Path) -> Result<Self> {
        let internal_writer = GeoParquetBatchWriter::new(
            output_path,
            BatchConfig {
                max_rows_per_batch: 100_000,
            },
        )?;
        Ok(GeoParquetWriter {
            internal_writer,
            output_path: output_path.to_path_buf(),
            has_features: false,
        })
    }

    fn add_xml_features(&mut self, parsed: ParsedXML) -> Result<()> {
        // Write each feature, consuming the parsed data
        for feature in parsed.features {
            self.has_features = true;
            let geometry: Polygon<f64> = feature.geometry.into();

            let FeatureProperties {
                筆id,
                精度区分,
                大字コード,
                丁目コード,
                小字コード,
                予備コード,
                大字名,
                丁目名,
                小字名,
                予備名,
                地番,
                座標値種別,
                筆界未定構成筆,
                代表点緯度,
                代表点経度,
            } = feature.props;

            let row = OutputRow {
                geom: geometry,
                地図名: parsed.common_props.地図名.clone(),
                市区町村コード: parsed.common_props.市区町村コード.clone(),
                市区町村名: parsed.common_props.市区町村名.clone(),
                座標系: parsed.common_props.座標系.clone(),
                測地系判別: parsed.common_props.測地系判別.clone(),
                筆id,
                精度区分,
                大字コード,
                丁目コード,
                小字コード,
                予備コード,
                大字名,
                丁目名,
                小字名,
                予備名,
                地番,
                座標値種別,
                代表点緯度,
                代表点経度,
                筆界未定構成筆,
            };
            self.internal_writer.add_row(row)?;
        }

        Ok(())
    }

    fn flush(self: Box<Self>) -> Result<bool> {
        if self.has_features {
            self.internal_writer.finish()?;
            Ok(true)
        } else {
            // Drop the writer to close the file before removing it
            drop(self.internal_writer);
            // Try to remove the file, ignore "not exists" errors
            match std::fs::remove_file(&self.output_path) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use geo_types::polygon;

    use crate::writer::shared::Writer;
    use mojxml_parser::{CommonProperties, Feature, FeatureProperties};

    use super::*;
    use std::path::PathBuf;

    fn testdata_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .join("testdata")
    }

    #[test]
    fn test_write_flatgeobuf() -> Result<()> {
        let parsed = ParsedXML {
            file_name: "test.xml".to_string(),
            features: vec![Feature {
                geometry: polygon![
                    (x: 0.0, y: 0.0),
                    (x: 1.0, y: 0.0),
                    (x: 1.0, y: 1.0),
                    (x: 0.0, y: 1.0),
                    (x: 0.0, y: 0.0)
                ],
                props: FeatureProperties::default(),
            }],
            common_props: CommonProperties {
                地図名: "テスト地図".to_string(),
                市区町村コード: "00000".to_string(),
                市区町村名: "テスト市".to_string(),
                座標系: "公共座標1系".to_string(),
                測地系判別: Some("変換".to_string()),
            },
        };
        let output_path = testdata_path().join("output.parquet");
        let mut writer = Box::new(GeoParquetWriter::new(&output_path)?);
        writer.add_xml_features(parsed)?;
        writer.flush()?;
        Ok(())
    }

    #[test]
    fn test_no_features_no_file() -> Result<()> {
        let parsed = ParsedXML {
            file_name: "test_empty.xml".to_string(),
            features: vec![], // Empty features array
            common_props: CommonProperties {
                地図名: "テスト地図".to_string(),
                市区町村コード: "00000".to_string(),
                市区町村名: "テスト市".to_string(),
                座標系: "公共座標1系".to_string(),
                測地系判別: Some("変換".to_string()),
            },
        };
        let output_path = testdata_path().join("output_empty.parquet");

        // Make sure the file doesn't exist before the test
        if output_path.exists() {
            std::fs::remove_file(&output_path)?;
        }

        let mut writer = Box::new(GeoParquetWriter::new(&output_path)?);
        writer.add_xml_features(parsed)?;
        writer.flush()?;

        // Verify the file was not created/was removed
        assert!(
            !output_path.exists(),
            "File should not exist when there are no features"
        );

        Ok(())
    }
}

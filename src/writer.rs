use crate::error::Error;
use crate::parse::ParsedXML;
use anyhow::Result;
use flatgeobuf::{
    ColumnType, FgbCrs, FgbWriter, FgbWriterOptions, GeometryType,
    geozero::{ColumnValue, PropertyProcessor},
};
use geo_types::Geometry;
use std::io::{BufWriter, Write};
use std::{fs::File, path::Path};

pub struct FGBWriter<'a> {
    fgb: FgbWriter<'a>,
    writer: BufWriter<File>,
}
impl FGBWriter<'_> {
    pub fn new(output_path: &Path) -> Result<Self> {
        let file = File::create(&output_path).map_err(|e| Error::Projection(e.to_string()))?;
        let writer = BufWriter::new(file);

        let mut fgb = FgbWriter::create_with_options(
            "mojxml",
            GeometryType::MultiPolygon,
            FgbWriterOptions {
                crs: FgbCrs {
                    code: 4326,
                    ..Default::default()
                },
                ..Default::default()
            },
        )?;
        fgb.add_column("地図名", ColumnType::String, |_, _| {});
        fgb.add_column("市区町村コード", ColumnType::String, |_, _| {});
        fgb.add_column("市区町村名", ColumnType::String, |_, _| {});
        fgb.add_column("座標系", ColumnType::String, |_, _| {});
        fgb.add_column("測地系判別", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("測地系", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("測地系変換", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("筆id", ColumnType::String, |_, _| {});
        fgb.add_column("精度区分", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("大字コード", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("丁目コード", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("小字コード", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("予備コード", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("大字名", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("丁目名", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("小字名", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("予備名", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("地番", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("座標値種別", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("筆界未定構成筆", ColumnType::String, |_, col| {
            col.nullable = true;
        });

        Ok(FGBWriter { fgb, writer })
    }

    pub fn add_xml_features(&mut self, parsed: ParsedXML) -> Result<()> {
        // Write each feature, consuming the parsed data
        for feature in parsed.features {
            let geometry: Geometry<f64> = feature.geometry.into();
            self.fgb.add_feature_geom(geometry, |feat| {
                feat.property(
                    0,
                    "地図名",
                    &ColumnValue::String(&parsed.common_props.地図名),
                )
                .unwrap();
                feat.property(
                    1,
                    "市区町村コード",
                    &ColumnValue::String(&parsed.common_props.市区町村コード),
                )
                .unwrap();
                feat.property(
                    2,
                    "市区町村名",
                    &ColumnValue::String(&parsed.common_props.市区町村名),
                )
                .unwrap();
                feat.property(
                    3,
                    "座標系",
                    &ColumnValue::String(&parsed.common_props.座標系),
                )
                .unwrap();
                if let Some(ref conversion) = parsed.common_props.測地系判別 {
                    feat.property(4, "測地系判別", &ColumnValue::String(conversion))
                        .unwrap();
                }
                feat.property(5, "筆id", &ColumnValue::String(&feature.props.筆id))
                    .unwrap();

                // only set optional properties if present, leave others null
                if let Some(v) = feature.props.精度区分.as_ref() {
                    feat.property(6, "精度区分", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.大字コード.as_ref() {
                    feat.property(7, "大字コード", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.丁目コード.as_ref() {
                    feat.property(8, "丁目コード", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.小字コード.as_ref() {
                    feat.property(9, "小字コード", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.予備コード.as_ref() {
                    feat.property(10, "予備コード", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.大字名.as_ref() {
                    feat.property(11, "大字名", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.丁目名.as_ref() {
                    feat.property(12, "丁目名", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.小字名.as_ref() {
                    feat.property(13, "小字名", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.予備名.as_ref() {
                    feat.property(14, "予備名", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.地番.as_ref() {
                    feat.property(15, "地番", &ColumnValue::String(v)).unwrap();
                }
                if let Some(v) = feature.props.座標値種別.as_ref() {
                    feat.property(16, "座標値種別", &ColumnValue::String(v))
                        .unwrap();
                }
                if let Some(v) = feature.props.筆界未定構成筆.as_ref() {
                    feat.property(17, "筆界未定構成筆", &ColumnValue::String(v))
                        .unwrap();
                }
            })?;
        }

        Ok(())
    }

    /// Flush the writer and finalize the FlatGeobuf file.
    /// This method must be called to ensure all data is written to the file.
    /// You cannot add any more features after calling this method.
    pub fn flush(mut self) -> Result<()> {
        self.fgb.write(&mut self.writer)?;
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use geo_types::{MultiPolygon, polygon};

    use crate::parse::{CommonProperties, Feature, FeatureProperties};

    use super::*;
    use std::path::PathBuf;

    fn testdata_path() -> PathBuf {
        PathBuf::from("testdata")
    }

    #[test]
    fn test_write_flatgeobuf() -> Result<()> {
        let parsed = ParsedXML {
            file_name: "test.xml".to_string(),
            features: vec![Feature {
                geometry: MultiPolygon::from(vec![polygon![
                    (x: 0.0, y: 0.0),
                    (x: 1.0, y: 0.0),
                    (x: 1.0, y: 1.0),
                    (x: 0.0, y: 1.0),
                    (x: 0.0, y: 0.0)
                ]]),
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
        let output_path = testdata_path().join("output.fgb");
        let mut fgb = FGBWriter::new(&output_path)?;
        fgb.add_xml_features(parsed)?;
        fgb.flush()?;
        Ok(())
    }
}

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::Result;
use flatgeobuf::{
    ColumnType, FgbCrs, FgbWriter, FgbWriterOptions, GeometryType,
    geozero::{ColumnValue, PropertyProcessor},
};

use crate::parse::ParsedXML;

pub struct FlatGeobufWriter<'a> {
    fgb: FgbWriter<'a>,
    writer: BufWriter<File>,
    output_path: PathBuf,
    has_features: bool,
}

impl<'a> crate::writer::shared::Writer for FlatGeobufWriter<'a> {
    fn new(output_path: &Path) -> Result<Self> {
        let file = File::create(output_path)?;
        let writer = BufWriter::new(file);

        let mut fgb = FgbWriter::create_with_options(
            "mojxml",
            GeometryType::MultiPolygon,
            FgbWriterOptions {
                crs: FgbCrs {
                    code: 4326,
                    ..Default::default()
                },
                write_index: true,
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
        fgb.add_column("筆id", ColumnType::String, |_, _| {});
        fgb.add_column("精度区分", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("大字コード", ColumnType::String, |_, col| {
            col.nullable = false;
        });
        fgb.add_column("丁目コード", ColumnType::String, |_, col| {
            col.nullable = false;
        });
        fgb.add_column("小字コード", ColumnType::String, |_, col| {
            col.nullable = false;
        });
        fgb.add_column("予備コード", ColumnType::String, |_, col| {
            col.nullable = false;
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
            col.nullable = false;
        });
        fgb.add_column("座標値種別", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("筆界未定構成筆", ColumnType::String, |_, col| {
            col.nullable = true;
        });
        fgb.add_column("代表点緯度", ColumnType::Double, |_, col| {
            col.nullable = false;
        });
        fgb.add_column("代表点経度", ColumnType::Double, |_, col| {
            col.nullable = false;
        });

        Ok(FlatGeobufWriter {
            fgb,
            writer,
            output_path: output_path.to_path_buf(),
            has_features: false,
        })
    }

    fn add_xml_features(&mut self, parsed: ParsedXML) -> Result<()> {
        // Write each feature, consuming the parsed data
        for feature in parsed.features {
            self.has_features = true;
            let geometry: geo_types::Geometry<f64> = feature.geometry.into();
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

                feat.property(
                    7,
                    "大字コード",
                    &ColumnValue::String(&feature.props.大字コード),
                )
                .unwrap();
                feat.property(
                    8,
                    "丁目コード",
                    &ColumnValue::String(&feature.props.丁目コード),
                )
                .unwrap();
                feat.property(
                    9,
                    "小字コード",
                    &ColumnValue::String(&feature.props.小字コード),
                )
                .unwrap();
                feat.property(
                    10,
                    "予備コード",
                    &ColumnValue::String(&feature.props.予備コード),
                )
                .unwrap();

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

                feat.property(15, "地番", &ColumnValue::String(&feature.props.地番))
                    .unwrap();

                if let Some(v) = feature.props.座標値種別.as_ref() {
                    feat.property(16, "座標値種別", &ColumnValue::String(v))
                        .unwrap();
                }

                if !feature.props.筆界未定構成筆.is_empty() {
                    let mitei = serde_json::to_string(&feature.props.筆界未定構成筆).unwrap();
                    feat.property(17, "筆界未定構成筆", &ColumnValue::String(&mitei))
                        .unwrap();
                }

                feat.property(
                    18,
                    "代表点緯度",
                    &ColumnValue::Double(feature.props.代表点緯度),
                )
                .unwrap();
                feat.property(
                    19,
                    "代表点経度",
                    &ColumnValue::Double(feature.props.代表点経度),
                )
                .unwrap();
            })?;
        }

        Ok(())
    }

    fn flush(mut self: Box<Self>) -> Result<bool> {
        if self.has_features {
            self.fgb.write(&mut self.writer)?;
            self.writer.flush()?;
            Ok(true)
        } else {
            // Drop the writer to close the file before removing it
            drop(self.writer);
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

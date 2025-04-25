use anyhow::Result;
use flatgeobuf::{FgbCrs, FgbWriter, FgbWriterOptions, GeometryType};
use std::io::{BufWriter, Write};
use std::marker::PhantomData;
use std::{fs::File, path::Path};

pub trait AsOption<'a, T> {
    fn as_option(&'a self) -> Option<T>;
}

impl<'a> AsOption<'a, &'a str> for String {
    fn as_option(&'a self) -> Option<&'a str> {
        Some(self.as_str())
    }
}

impl<'a> AsOption<'a, &'a str> for Option<String> {
    fn as_option(&'a self) -> Option<&'a str> {
        self.as_deref()
    }
}

impl<'a> AsOption<'a, u32> for u32 {
    fn as_option(&'a self) -> Option<u32> {
        Some(*self)
    }
}

impl<'a> AsOption<'a, u32> for Option<u32> {
    fn as_option(&'a self) -> Option<u32> {
        *self
    }
}

impl<'a> AsOption<'a, f64> for f64 {
    fn as_option(&'a self) -> Option<f64> {
        Some(*self)
    }
}

impl<'a> AsOption<'a, f64> for Option<f64> {
    fn as_option(&'a self) -> Option<f64> {
        *self
    }
}

pub trait FgbColumnar {
    /// Call once before writing any rows:
    fn register_columns(fgb: &mut FgbWriter);
    /// Call per-record to append its properties:
    fn write_feature(&self, fgb: &mut FgbWriter);
}

#[macro_export]
macro_rules! impl_fgb_columnar {
    (
        for $ty:ident {
            $(
                { name: $col_name:expr, field: $field:ident, ctype: $ctype:ident, nullable: $nullable:literal }
            ),* $(,)?
        }
    ) => {
        impl $crate::writer::FgbColumnar for $ty {
            fn register_columns(fgb: &mut flatgeobuf::FgbWriter) {
                $(
                    fgb.add_column($col_name, flatgeobuf::ColumnType::$ctype, |_, c| {
                        c.nullable = $nullable;
                    });
                )*
            }

            fn write_feature(&self, fgb: &mut flatgeobuf::FgbWriter) {
                use flatgeobuf::geozero::PropertyProcessor;
                use $crate::writer::AsOption;

                let geometry: geo_types::Geometry<f64> = self.geometry.clone().into();
                let _ = fgb.add_feature_geom(geometry, |feat| {
                    let mut _idx = 0;
                    $(
                        if let Some(val) = self.props.$field.as_option() {
                            feat.property(_idx, $col_name, &flatgeobuf::geozero::ColumnValue::$ctype(val)).unwrap();
                        }
                        _idx += 1;
                    )*
                });
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct WriterOptions {
    pub write_index: bool,
}

pub struct FGBWriter<'a, T: FgbColumnar> {
    fgb: FgbWriter<'a>,
    writer: BufWriter<File>,
    phantom: PhantomData<T>,
}
impl<T> FGBWriter<'_, T>
where
    T: FgbColumnar,
{
    pub fn new(output_path: &Path, options: &WriterOptions) -> Result<Self> {
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
                write_index: options.write_index,
                ..Default::default()
            },
        )?;
        T::register_columns(&mut fgb);

        Ok(FGBWriter::<T> {
            fgb,
            writer,
            phantom: PhantomData,
        })
    }

    pub fn add_features(&mut self, features: &[T]) -> Result<()> {
        for feature in features {
            feature.write_feature(&mut self.fgb);
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
    use super::*;
    use crate::parse::{Feature, FeatureProperties, ParsedXML};
    use geo_types::{MultiPolygon, polygon};
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
        };
        let output_path = testdata_path().join("output.fgb");
        let mut fgb = FGBWriter::new(&output_path, &WriterOptions { write_index: true })?;
        fgb.add_features(&parsed.features)?;
        fgb.flush()?;
        Ok(())
    }
}

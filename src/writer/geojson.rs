use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::Result;
use geojson::GeoJson;
use serde::Serialize;

use crate::parse::ParsedXML;

/// A GeoJSON writer
///
/// Note, this outputs newline-delimited GeoJSON.
pub struct GeoJsonWriter {
    bufwrite: BufWriter<File>,
}

impl crate::writer::shared::Writer for GeoJsonWriter {
    fn new(output_path: &Path) -> Result<Self> {
        let file = File::create(output_path)?;
        let bufwrite = BufWriter::new(file);
        Ok(GeoJsonWriter { bufwrite })
    }

    fn add_xml_features(&mut self, parsed: ParsedXML) -> Result<()> {
        for feature in &parsed.features {
            let props = OutputProps {
                shared: &parsed.common_props,
                feature: &feature.props,
            };
            let props = match serde_json::to_value(&props)? {
                serde_json::Value::Object(map) => map,
                _ => panic!("expected object"),
            };
            let gf = GeoJson::Feature(geojson::Feature {
                id: None,
                bbox: None,
                geometry: Some(geojson::Value::from(&feature.geometry).into()),
                properties: Some(props),
                foreign_members: None,
            });

            self.bufwrite.write(gf.to_string().as_bytes())?;
            self.bufwrite.write(b"\n")?;
        }
        Ok(())
    }

    fn flush(mut self: Box<Self>) -> Result<bool> {
        self.bufwrite.flush()?;
        Ok(true)
    }
}

#[derive(Serialize)]
struct OutputProps<'a> {
    #[serde(flatten)]
    shared: &'a crate::parse::CommonProperties,
    #[serde(flatten)]
    feature: &'a crate::parse::FeatureProperties,
}

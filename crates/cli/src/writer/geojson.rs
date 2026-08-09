use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::Result;
use geojson::{Geometry, GeometryValue};
use serde::Serialize;

use mojxml_parser::{CommonProperties, FeatureProperties, ParsedXML};

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
        let shared = &parsed.common_props;
        for feature in &parsed.features {
            // Stream one feature per line directly to the buffered writer to avoid
            // intermediate serde_json::Value/Object and String allocations.
            let output_feature = OutputFeature {
                feature_type: "Feature",
                geometry: Geometry::new(GeometryValue::from(&feature.geometry)),
                properties: OutputProps {
                    shared,
                    feature: &feature.props,
                },
            };

            serde_json::to_writer(&mut self.bufwrite, &output_feature)?;
            self.bufwrite.write_all(b"\n")?;
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
    shared: &'a CommonProperties,
    #[serde(flatten)]
    feature: &'a FeatureProperties,
}

#[derive(Serialize)]
struct OutputFeature<'a> {
    #[serde(rename = "type")]
    feature_type: &'static str,
    geometry: Geometry,
    properties: OutputProps<'a>,
}

use serde::Serialize;
use wasm_bindgen::prelude::*;
// use web_sys::console;

#[wasm_bindgen]
pub fn parse_xml_content(file_name: &str, xml_content: &str) -> Result<JsValue, JsValue> {
    let parse_options = mojxml_parser::ParseOptions {
        include_arbitrary_crs: false,
        include_chikugai: false,
    };
    let parsed = mojxml_parser::parse_xml_content(file_name, xml_content, &parse_options);
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(e) => return Err(format!("Error: {:?}", e).into()),
    };

    let mut features = Vec::with_capacity(parsed.features.len());
    for feature in parsed.features {
        let mojxml_parser::Feature { geometry, props } = feature;

        let properties = match serde_json::to_value(props) {
            Ok(serde_json::Value::Object(map)) => Some(map),
            Ok(_) => None,
            Err(err) => {
                return Err(JsValue::from_str(&format!(
                    "Failed to serialize feature properties: {err}"
                )));
            }
        };

        features.push(geojson::Feature {
            bbox: None,
            geometry: Some((&geometry).into()),
            id: None,
            properties,
            foreign_members: None,
        });
    }

    let geojson = geojson::FeatureCollection {
        features,
        bbox: None,
        foreign_members: None,
    };

    geojson
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|err| JsValue::from_str(&format!("Error serializing GeoJSON: {err}")))
}

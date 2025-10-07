use wasm_bindgen::prelude::*;
// use web_sys::console;

#[wasm_bindgen]
pub fn parse_xml_content(file_name: &str, xml_content: &str) -> JsValue {
    let parse_options = mojxml_parser::ParseOptions {
        include_arbitrary_crs: true,
        include_chikugai: true,
    };
    let parsed = mojxml_parser::parse_xml_content(
        &(file_name.to_string(), xml_content.to_string()),
        &parse_options,
    );
    match parsed {
        Ok(parsed) => {
            let geojson = geojson::FeatureCollection {
                features: parsed
                    .features
                    .into_iter()
                    .map(|f| {
                        let properties = serde_json::to_value(&f.props)
                            .expect("Failed to serialize properties")
                            .as_object()
                            .cloned();
                        geojson::Feature {
                            bbox: None,
                            geometry: Some((&f.geometry).into()),
                            id: None,
                            properties,
                            foreign_members: None,
                        }
                    })
                    .collect(),
                bbox: None,
                foreign_members: None,
            };
            serde_wasm_bindgen::to_value(&geojson).unwrap()
        }
        Err(e) => {
            // console::error_1(&JsValue::from_str(&format!("Error: {:?}", e)));
            JsValue::from_str(&format!("Error: {:?}", e))
        }
    }
}

use crate::error::Result;
use crate::impl_fgb_columnar;
use crate::parse::ParsedXML;
use geo::algorithm::ConcaveHull;
use geo_types::MultiPolygon;

/// Represents a feature that is the outline of multiple features
#[derive(Debug, Clone)]
pub struct OutlineFeature {
    pub geometry: MultiPolygon,
    pub props: OutlineFeatureProperties,
}

/// Properties for a outline feature
#[derive(Debug, Clone)]
pub struct OutlineFeatureProperties {
    // Common properties from CommonProperties
    pub 地図名: String,
    pub 市区町村コード: u32,
    pub 市区町村名: String,
    pub 座標系: String,
    pub 測地系判別: Option<String>,

    // Additional properties for outline
    pub count: u32,
}

impl_fgb_columnar! {
    for OutlineFeature {
        { name: "地図名", field: 地図名, ctype: String, nullable: false },
        { name: "市区町村コード", field: 市区町村コード, ctype: UInt, nullable: false },
        { name: "市区町村名", field: 市区町村名, ctype: String, nullable: false },
        { name: "座標系", field: 座標系, ctype: String, nullable: false },
        { name: "測地系判別", field: 測地系判別, ctype: String, nullable: true },
        { name: "count", field: count, ctype: UInt, nullable: false },
    }
}

/// Calculate the outline of all features in a ParsedXML struct
///
/// This function combines all geometries into a single MultiPolygon and creates
/// a feature that represents the outline, as determined by the concave hull.
/// Common properties are preserved, and a count property is added to indicate
/// the number of features in the outline.
///
/// # Arguments
///
/// * `parsed_xml` - The ParsedXML struct containing features to outline
///
/// # Returns
///
/// * `Result<OutlineFeature>` - A feature representing the outline of all features
pub fn calculate_feature_outline(parsed_xml: &ParsedXML) -> Result<OutlineFeature> {
    if parsed_xml.features.is_empty() {
        return Err(crate::error::Error::MissingElement(
            "No features to calculate outline".to_string(),
        ));
    }

    // Get the first feature to extract common properties
    let first_feature = &parsed_xml.features[0];

    // Create a MultiPolygon of all the multipolygons from the features
    let all_geometries = MultiPolygon(
        parsed_xml
            .features
            .iter()
            .flat_map(|mp| mp.geometry.0.clone())
            .collect(),
    );
    // Calculate the concave hull of the combined geometry
    let outline_geometry = all_geometries.concave_hull(1.0);

    // Create the outline feature with common properties and the count
    let outline_feature = OutlineFeature {
        geometry: outline_geometry.into(),
        props: OutlineFeatureProperties {
            地図名: first_feature.props.地図名.clone(),
            市区町村コード: first_feature.props.市区町村コード,
            市区町村名: first_feature.props.市区町村名.clone(),
            座標系: first_feature.props.座標系.clone(),
            測地系判別: first_feature.props.測地系判別.clone(),
            count: parsed_xml.features.len() as u32,
        },
    };

    Ok(outline_feature)
}

#[cfg(test)]
mod tests {
    use crate::parse::{Feature, FeatureProperties};

    use super::*;
    use geo_types::{Coord, LineString, Polygon};

    #[test]
    fn test_calculate_feature_outline() {
        // Create test features with different geometries
        let feature1 = create_test_feature(0.0, 0.0, 1.0, 1.0);
        let feature2 = create_test_feature(0.5, 0.5, 1.5, 1.5);

        // Create a ParsedXML with these features
        let parsed_xml = ParsedXML {
            file_name: "test.xml".to_string(),
            features: vec![feature1, feature2],
        };

        // Calculate the outline
        let outline_feature = calculate_feature_outline(&parsed_xml).unwrap();

        // Check the count property
        assert_eq!(outline_feature.props.count, 2);

        // The outline should cover both original features
        // For a proper test, we could check specific properties of the geometry
        // but for simplicity, we'll just check that it contains at least one polygon
        assert!(!outline_feature.geometry.0.is_empty());
    }

    // Helper function to create a test feature with a rectangular polygon
    fn create_test_feature(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Feature {
        let polygon = Polygon::new(
            LineString::from(vec![
                Coord { x: min_x, y: min_y },
                Coord { x: max_x, y: min_y },
                Coord { x: max_x, y: max_y },
                Coord { x: min_x, y: max_y },
                Coord { x: min_x, y: min_y },
            ]),
            vec![],
        );

        let multi_polygon = MultiPolygon::new(vec![polygon]);

        Feature {
            geometry: multi_polygon,
            props: FeatureProperties {
                地図名: "Test Map".to_string(),
                市区町村コード: 12345,
                市区町村名: "Test City".to_string(),
                座標系: "WGS84".to_string(),
                測地系判別: Some("Test".to_string()),
                筆id: "test-id".to_string(),
                精度区分: None,
                大字コード: None,
                丁目コード: None,
                小字コード: None,
                予備コード: None,
                大字名: None,
                丁目名: None,
                小字名: None,
                予備名: None,
                地番: None,
                座標値種別: None,
                筆界未定構成筆: None,
                代表点緯度: 0.5,
                代表点経度: 0.5,
            },
        }
    }
}

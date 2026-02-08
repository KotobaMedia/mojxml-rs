use crate::constants::get_proj;
use crate::error::{Error, Result};
use crate::types::{CommonProperties, Feature, FeatureProperties};
use crate::{ParsedXML, 筆界未定構成筆};
use geo::algorithm::interior_point::InteriorPoint;
use geo_types::{LineString, Point, Polygon};
use proj4rs::proj::Proj;
use roxmltree::{Document, Node};
use rustc_hash::FxHashMap as HashMap;

// --- Type Aliases ---
type Curve = Point;
type Surface = Polygon;

fn has_name(node: &Node, name: &str) -> bool {
    node.tag_name().name() == name
}

fn find_child_by_name<'a, 'd>(node: &Node<'a, 'd>, name: &str) -> Option<Node<'a, 'd>>
where
    'd: 'a,
{
    node.children()
        .find(|child| child.is_element() && has_name(child, name))
}

fn required_attribute<'a, 'd>(node: &Node<'a, 'd>, attr: &str) -> Result<&'a str>
where
    'd: 'a,
{
    node.attribute(attr).ok_or_else(|| Error::MissingAttribute {
        element: node.tag_name().name().to_string(),
        attribute: attr.to_string(),
    })
}

fn node_text(node: &Node, label: &str) -> Result<String> {
    node.text()
        .map(|text| text.to_string())
        .ok_or_else(|| Error::MissingElement(label.to_string()))
}

fn child_text(node: &Node, label: &str) -> Result<String> {
    let child = get_child_element(node, label)?;
    node_text(&child, label)
}

fn parse_text_as_f64(node: &Node, label: &str) -> Result<f64> {
    let text = node
        .text()
        .ok_or_else(|| Error::MissingElement(label.to_string()))?;
    Ok(text.parse::<f64>()?)
}

fn parse_xy(node: &Node) -> Result<(f64, f64)> {
    let mut x = None;
    let mut y = None;

    for child in node.children().filter(|child| child.is_element()) {
        match child.tag_name().name() {
            "X" => x = Some(parse_text_as_f64(&child, "X")?),
            "Y" => y = Some(parse_text_as_f64(&child, "Y")?),
            _ => {}
        }
    }

    let x = x.ok_or_else(|| Error::MissingElement("X".to_string()))?;
    let y = y.ok_or_else(|| Error::MissingElement("Y".to_string()))?;
    Ok((x, y))
}

fn collect_ring_points<'a, 'd>(
    boundary: &Node<'a, 'd>,
    curves: &HashMap<&'a str, Curve>,
) -> Result<Vec<Point>>
where
    'd: 'a,
{
    let mut ring_points = Vec::new();

    for ring in boundary
        .children()
        .filter(|child| child.is_element() && has_name(child, "GM_Ring"))
    {
        for curve_ref in ring.children().filter(|child| child.is_element()) {
            let idref = required_attribute(&curve_ref, "idref")?;
            let curve = curves
                .get(idref)
                .ok_or_else(|| Error::PointNotFound(idref.to_string()))?;
            ring_points.push(*curve);
        }
    }

    Ok(ring_points)
}

fn parse_constituent_fude(node: &Node) -> 筆界未定構成筆 {
    let mut constituent = 筆界未定構成筆::default();

    for entry in node.children().filter(|child| child.is_element()) {
        let tag_name = entry.tag_name().name();
        let text = entry.text();
        match tag_name {
            "大字コード" => constituent.大字コード = text.unwrap_or("").to_owned(),
            "丁目コード" => constituent.丁目コード = text.unwrap_or("").to_owned(),
            "小字コード" => constituent.小字コード = text.unwrap_or("").to_owned(),
            "予備コード" => constituent.予備コード = text.unwrap_or("").to_owned(),
            "大字名" => constituent.大字名 = text.map(str::to_owned),
            "丁目名" => constituent.丁目名 = text.map(str::to_owned),
            "小字名" => constituent.小字名 = text.map(str::to_owned),
            "予備名" => constituent.予備名 = text.map(str::to_owned),
            "地番" => constituent.地番 = text.unwrap_or("").to_owned(),
            _ => {}
        }
    }

    constituent
}

fn point_on_polygon(polygon: &Polygon) -> Result<Point<f64>> {
    // interior_point returns None if the polygon is empty or has no interior point
    // We've tested on 2024 data, and all polygons have an interior point
    polygon
        .interior_point()
        .ok_or(Error::InteriorPointUnavailable)
}

#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    pub include_arbitrary_crs: bool,
    pub include_chikugai: bool,
}

// --- Helper Functions ---
fn get_child_element<'a, 'd>(node: &Node<'a, 'd>, name: &str) -> Result<Node<'a, 'd>>
where
    'd: 'a,
{
    node.children()
        .find(|child| child.tag_name().name() == name)
        .ok_or_else(|| Error::MissingElement(name.to_string()))
}

// -- Accessory parsing functions --
fn parse_points<'a, 'd>(spatial_element: &Node<'a, 'd>) -> Result<HashMap<&'a str, Point>>
where
    'd: 'a,
{
    let mut points: HashMap<&'a str, Point> = HashMap::default();

    for point in spatial_element
        .children()
        .filter(|child| child.is_element() && has_name(child, "GM_Point"))
    {
        let position_node = find_child_by_name(&point, "GM_Point.position")
            .ok_or_else(|| Error::MissingElement("GM_Point.position".to_string()))?;
        let direct_position = find_child_by_name(&position_node, "DirectPosition")
            .ok_or_else(|| Error::MissingElement("DirectPosition".to_string()))?;
        let (x, y) = parse_xy(&direct_position)?;
        let point_id = required_attribute(&point, "id")?;
        points.insert(point_id, Point::new(x, y));
    }

    Ok(points)
}

fn parse_curves<'a, 'd>(
    spatial_element: &Node<'a, 'd>,
    points: &HashMap<&'a str, Point>,
) -> Result<HashMap<&'a str, Curve>>
where
    'd: 'a,
{
    let mut curves: HashMap<&'a str, Curve> = HashMap::default();

    for curve in spatial_element
        .children()
        .filter(|child| child.is_element() && has_name(child, "GM_Curve"))
    {
        let curve_id = required_attribute(&curve, "id")?;

        let segment = curve
            .children()
            .find(|child| child.is_element() && has_name(child, "GM_Curve.segment"))
            .ok_or_else(|| Error::MissingElement("GM_Curve.segment".to_string()))?;

        let column = find_child_by_name(&segment, "GM_LineString")
            .and_then(|line| find_child_by_name(&line, "GM_LineString.controlPoint"))
            .and_then(|control| find_child_by_name(&control, "GM_PointArray.column"))
            .ok_or_else(|| Error::MissingElement("GM_PointArray.column".to_string()))?;

        let position = column
            .first_element_child()
            .ok_or_else(|| Error::MissingElement("GM_Position.*".to_string()))?;

        let (x, y) = match position.tag_name().name() {
            "GM_Position.indirect" => {
                let reference = position
                    .first_element_child()
                    .ok_or_else(|| Error::MissingElement("GM_Position.indirect".to_string()))?;
                let idref = required_attribute(&reference, "idref")?;
                let point = points
                    .get(idref)
                    .ok_or_else(|| Error::PointNotFound(idref.to_string()))?;
                (point.x(), point.y())
            }
            "GM_Position.direct" => parse_xy(&position)?,
            other => return Err(Error::UnexpectedElement(other.to_string())),
        };

        curves.insert(curve_id, Curve::new(y, x));
    }

    Ok(curves)
}

/// Transform all curves' coordinates from source_crs to target_crs in-place.
fn transform_curves_crs(
    curves: &mut HashMap<&str, Curve>,
    source_crs: &Proj,
    target_crs: &Proj,
) -> Result<()> {
    if curves.is_empty() {
        return Ok(());
    }

    for curve in curves.values_mut() {
        let mut point = curve.x_y();
        proj4rs::transform::transform(source_crs, target_crs, &mut point)?;
        *curve = Point::new(point.0.to_degrees(), point.1.to_degrees());
    }

    Ok(())
}

fn parse_surfaces<'a, 'd>(
    spatial_element: &Node<'a, 'd>,
    curves: &HashMap<&'a str, Curve>,
) -> Result<HashMap<&'a str, Surface>>
where
    'd: 'a,
{
    let mut surfaces: HashMap<&'a str, Surface> = HashMap::default();

    for surface in spatial_element
        .children()
        .filter(|child| child.is_element() && has_name(child, "GM_Surface"))
    {
        let surface_id = required_attribute(&surface, "id")?;

        let polygon = find_child_by_name(&surface, "GM_Surface.patch")
            .and_then(|patch| find_child_by_name(&patch, "GM_Polygon"))
            .ok_or_else(|| Error::MissingElement("GM_Surface.patch".to_string()))?;

        let surface_boundary = find_child_by_name(&polygon, "GM_Polygon.boundary")
            .and_then(|boundary| find_child_by_name(&boundary, "GM_SurfaceBoundary"))
            .ok_or_else(|| Error::MissingElement("GM_SurfaceBoundary".to_string()))?;

        let exterior = find_child_by_name(&surface_boundary, "GM_SurfaceBoundary.exterior")
            .ok_or_else(|| Error::MissingElement("GM_SurfaceBoundary.exterior".to_string()))?;

        let exterior_ring = LineString::from(collect_ring_points(&exterior, curves)?);

        let mut interior_rings = Vec::new();
        for interior in surface_boundary
            .children()
            .filter(|child| child.is_element() && has_name(child, "GM_SurfaceBoundary.interior"))
        {
            interior_rings.push(LineString::from(collect_ring_points(&interior, curves)?));
        }

        surfaces.insert(surface_id, Polygon::new(exterior_ring, interior_rings));
    }

    Ok(surfaces)
}

fn parse_features<'a, 'd>(
    subject_elem: &Node<'a, 'd>,
    surfaces: &HashMap<&'a str, Surface>,
    options: &ParseOptions,
) -> Result<Vec<Feature>>
where
    'd: 'a,
{
    let mut features: Vec<Feature> = Vec::new();

    for fude in subject_elem
        .children()
        .filter(|child| child.is_element() && has_name(child, "筆"))
    {
        let fude_id = required_attribute(&fude, "id")?;
        let mut geometry: Option<Polygon> = None;

        let mut 精度区分 = None;
        let mut 大字コード = None;
        let mut 丁目コード = None;
        let mut 小字コード = None;
        let mut 予備コード = None;
        let mut 大字名 = None;
        let mut 丁目名 = None;
        let mut 小字名 = None;
        let mut 予備名 = None;
        let mut 地番 = None;
        let mut 座標値種別 = None;
        let mut 筆界未定構成筆 = Vec::new();

        for entry in fude.children().filter(|child| child.is_element()) {
            let tag_name = entry.tag_name().name();
            match tag_name {
                "形状" => {
                    let idref = required_attribute(&entry, "idref")?;
                    geometry = surfaces.get(idref).cloned();
                }
                "精度区分" => 精度区分 = entry.text().map(str::to_owned),
                "大字コード" => 大字コード = Some(entry.text().unwrap_or("").to_owned()),
                "丁目コード" => 丁目コード = Some(entry.text().unwrap_or("").to_owned()),
                "小字コード" => 小字コード = Some(entry.text().unwrap_or("").to_owned()),
                "予備コード" => 予備コード = Some(entry.text().unwrap_or("").to_owned()),
                "大字名" => 大字名 = entry.text().map(str::to_owned),
                "丁目名" => 丁目名 = entry.text().map(str::to_owned),
                "小字名" => 小字名 = entry.text().map(str::to_owned),
                "予備名" => 予備名 = entry.text().map(str::to_owned),
                "地番" => 地番 = Some(entry.text().unwrap_or("").to_owned()),
                "座標値種別" => 座標値種別 = entry.text().map(str::to_owned),
                "筆界未定構成筆" => 筆界未定構成筆.push(parse_constituent_fude(&entry)),
                _ => {}
            }
        }

        if !options.include_chikugai {
            match 地番.as_ref() {
                Some(value) if value.contains("地区外") || value.contains("別図") => continue,
                Some(_) => {}
                None => return Err(Error::MissingElement("地番".to_string())),
            }
        }

        let geometry = geometry.ok_or_else(|| Error::MissingElement("geometry".to_string()))?;
        let 大字コード =
            大字コード.ok_or_else(|| Error::MissingElement("大字コード".to_string()))?;
        let 丁目コード =
            丁目コード.ok_or_else(|| Error::MissingElement("丁目コード".to_string()))?;
        let 小字コード =
            小字コード.ok_or_else(|| Error::MissingElement("小字コード".to_string()))?;
        let 予備コード =
            予備コード.ok_or_else(|| Error::MissingElement("予備コード".to_string()))?;
        let 地番 = 地番.ok_or_else(|| Error::MissingElement("地番".to_string()))?;

        let pop = point_on_polygon(&geometry)?;
        features.push(Feature {
            geometry,
            props: FeatureProperties {
                筆id: fude_id.to_owned(),
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
                代表点緯度: pop.y(),
                代表点経度: pop.x(),
            },
        });
    }

    Ok(features)
}

fn parse_base_properties(root: &Node) -> Result<CommonProperties> {
    let map_name = child_text(root, "地図名")?;
    let city_code = child_text(root, "市区町村コード")?;
    let city_name = child_text(root, "市区町村名")?;
    let crs = child_text(root, "座標系")?;
    let crs_det = get_child_element(root, "測地系判別")
        .ok()
        .and_then(|elem| elem.text().map(|text| text.to_string()));

    Ok(CommonProperties {
        地図名: map_name,
        市区町村コード: city_code,
        市区町村名: city_name,
        座標系: crs,
        測地系判別: crs_det,
    })
}

// --- Main Parsing Function ---
pub fn parse_xml_content(
    file_name: &str,
    file_data: &str,
    options: &ParseOptions,
) -> Result<ParsedXML> {
    let file_name = file_name.to_string();
    let doc = Document::parse(file_data)?;
    let root = doc.root_element();

    let common_props = parse_base_properties(&root)?;

    let crs = get_proj(&common_props.座標系)?;
    if crs.is_none() && !options.include_arbitrary_crs {
        return Ok(ParsedXML {
            file_name,
            features: vec![],
            common_props,
        });
    }

    let spatial_element = get_child_element(&root, "空間属性")?;
    let points = parse_points(&spatial_element)?;
    let mut curves = parse_curves(&spatial_element, &points)?;
    if let Some(crs) = crs {
        let tgt_crs = get_proj("WGS84")?.expect("WGS84 CRS not found");
        transform_curves_crs(&mut curves, crs, tgt_crs)?;
    }

    let surfaces = parse_surfaces(&spatial_element, &curves)?;
    let subject_elem = get_child_element(&root, "主題属性")?;

    let features = parse_features(&subject_elem, &surfaces, options)?;
    Ok(ParsedXML {
        file_name,
        features,
        common_props,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::get_proj;
    use geo::Contains;
    use geo::{Area, BooleanOps};
    use geo_types::wkt;
    use rustc_hash::FxHashMap as HashMap;
    use std::fs;
    use std::path::PathBuf;

    fn testdata_path() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .join("testdata")
    }

    #[test]
    fn test_transform_curves_crs_public_coords_to_wgs84() {
        let source_crs = get_proj("公共座標1系")
            .expect("failed to load source CRS")
            .expect("公共座標1系 should resolve to a proj definition");
        let target_crs = get_proj("WGS84")
            .expect("failed to load target CRS")
            .expect("WGS84 should resolve to a proj definition");

        let mut curves: HashMap<&str, Curve> = [
            ("curve-1", Point::new(0.0, 0.0)),
            ("curve-2", Point::new(-1000.0, -1000.0)),
            ("curve-3", Point::new(1000.0, 1000.0)),
        ]
        .into_iter()
        .collect();

        let expected_results: HashMap<&str, Curve> = [
            ("curve-1", Point::new(129.5, 33.0)),
            ("curve-2", Point::new(129.48929948, 32.99098186)),
            ("curve-3", Point::new(129.5107027, 33.00901721)),
        ]
        .into_iter()
        .collect();

        transform_curves_crs(&mut curves, source_crs, target_crs)
            .expect("curve transformation should succeed");

        for (id, expected_point) in expected_results {
            let curve = curves.get(id).expect("transformed curve missing");
            assert!(
                (curve.x() - expected_point.x()).abs() < 1e-7,
                "longitude mismatch for {id} ({} vs {} )",
                curve.x(),
                expected_point.x()
            );
            assert!(
                (curve.y() - expected_point.y()).abs() < 1e-7,
                "latitude mismatch for {id} ({} vs {} )",
                curve.y(),
                expected_point.y()
            );
        }
    }

    #[test]
    fn test_parse_xml_content() {
        // Construct the path relative to the Cargo manifest directory
        let xml_path = testdata_path().join("46505-3411-56.xml");
        let xml_temp = fs::read_to_string(xml_path).expect("Failed to read XML file");
        let options = ParseOptions {
            include_arbitrary_crs: true,
            include_chikugai: true,
        };
        let ParsedXML {
            file_name: _,
            features,
            common_props,
        } = parse_xml_content("46505-3411-56.xml", &xml_temp, &options)
            .expect("Failed to parse XML");
        assert_eq!(common_props.地図名, "AYA1anbou22B04_2000");
        assert_eq!(common_props.市区町村コード, "46505");
        assert_eq!(common_props.市区町村名, "熊毛郡屋久島町");

        assert_eq!(features.len(), 2994);
        let feature = &features[0];
        assert_eq!(feature.props.筆id, "H000000001");
        assert_eq!(feature.props.地番, "1");

        let expected_geom = wkt! { POLYGON((130.65198936727597 30.31578177961301,130.65211112748588 30.31578250940004,130.65219722479674 30.315750035783307,130.6522397846286 30.315738240687146,130.65232325284867 30.315702331871517,130.6523668021 30.315675347347664,130.65235722919192 30.315650702546424,130.65229088479316 30.315622397556787,130.65227074994843 30.315602911975944,130.65225984787858 30.31558659939628,130.65223178039858 30.315557954059944,130.65219646886888 30.31555482900659,130.65216213192443 30.315543677500482,130.65214529987352 30.315560610998826,130.6521265046212 30.315576961906185,130.6521020960529 30.315589887800154,130.65207800626484 30.315597933967023,130.65192456437038 30.315643904777097,130.65190509850768 30.3156499243803,130.65198936727597 30.31578177961301)) };
        let difference = feature.geometry.difference(&expected_geom);
        assert!(
            difference.unsigned_area() < 1e-10,
            "Geometries do not match"
        );
    }

    #[test]
    fn test_parse_chikugai_miten_kosei_features() {
        // Test parsing of 筆界未定構成筆 elements
        let xml_path = testdata_path().join("46505-3411-56.xml");
        let xml_temp = fs::read_to_string(xml_path).expect("Failed to read XML file");
        let options = ParseOptions {
            include_arbitrary_crs: true,
            include_chikugai: true,
        };
        let ParsedXML {
            file_name: _,
            features,
            common_props: _,
        } = parse_xml_content("46505-3411-56.xml", &xml_temp, &options)
            .expect("Failed to parse XML");

        // Find a feature with 筆界未定構成筆 data
        let features_with_chikugai: Vec<_> = features
            .iter()
            .filter(|f| !f.props.筆界未定構成筆.is_empty())
            .collect();

        assert!(
            !features_with_chikugai.is_empty(),
            "Should find features with 筆界未定構成筆"
        );

        // Check the first feature with 筆界未定構成筆
        let feature_with_chikugai = features_with_chikugai[0];
        assert!(!feature_with_chikugai.props.筆界未定構成筆.is_empty());

        // Verify the structure of the first 筆界未定構成筆 element
        let first_constituent = &feature_with_chikugai.props.筆界未定構成筆[0];

        // These should not be empty/default based on the XML we saw
        assert!(!first_constituent.大字コード.is_empty());
        assert!(!first_constituent.地番.is_empty());
        assert!(first_constituent.大字名.is_some());

        println!(
            "Found feature with {} 筆界未定構成筆 elements",
            feature_with_chikugai.props.筆界未定構成筆.len()
        );
        println!(
            "First constituent: {} {} {}",
            first_constituent
                .大字名
                .as_ref()
                .unwrap_or(&"N/A".to_string()),
            first_constituent.地番,
            first_constituent.大字コード
        );
    }

    #[test]
    fn test_representative_point_should_be_inside_of_polygon() {
        // Construct the path relative to the Cargo manifest directory
        let xml_path = testdata_path().join("46505-3411-56.xml");
        let xml_temp = fs::read_to_string(xml_path).expect("Failed to read XML file");
        let options = ParseOptions {
            include_arbitrary_crs: false,
            include_chikugai: false,
        };
        let ParsedXML {
            file_name: _,
            features,
            common_props: _,
        } = parse_xml_content("46505-3411-56.xml", &xml_temp, &options)
            .expect("Failed to parse XML");

        for feature in features.iter() {
            let rep_point = Point::new(feature.props.代表点経度, feature.props.代表点緯度);
            let is_inside = feature.geometry.contains(&rep_point);
            assert!(is_inside, "Representative point is outside of the polygon");
        }
    }
}

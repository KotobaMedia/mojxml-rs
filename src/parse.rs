use crate::constants::{get_epsg_code, get_xml_namespace};
use crate::error::{Error, Result};
use crate::reader::FileData;
use geo_types::{LineString, MultiPolygon, Point, Polygon};
use proj::Proj;
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::vec;

// --- Type Aliases ---
type Curve = Point;
type Surface = MultiPolygon;

#[derive(Debug, Clone)]
pub struct Feature {
    pub geometry: MultiPolygon,
    pub props: FeatureProperties,
}

#[derive(Debug, Clone, Default)]
pub struct FeatureProperties {
    pub 筆id: String,
    pub 精度区分: Option<String>,
    pub 大字コード: Option<String>,
    pub 丁目コード: Option<String>,
    pub 小字コード: Option<String>,
    pub 予備コード: Option<String>,
    pub 大字名: Option<String>,
    pub 丁目名: Option<String>,
    pub 小字名: Option<String>,
    pub 予備名: Option<String>,
    pub 地番: Option<String>,
    pub 座標値種別: Option<String>,
    pub 筆界未定構成筆: Option<String>,
}

pub struct CommonProperties {
    pub 地図名: String,
    pub 市区町村コード: String,
    pub 市区町村名: String,
    pub 座標系: String,
    pub 測地系判別: Option<String>,
}

// --- Constants ---
const TARGET_CRS: &str = "EPSG:4326"; // WGS84

#[derive(Debug, Clone)]
pub struct ParseOptions {
    pub include_arbitrary_crs: bool,
    pub include_chikugai: bool,
}

// --- Helper Functions ---
fn get_child_element<'a>(node: &'a Node<'a, 'a>, name: &str) -> Result<Node<'a, 'a>> {
    node.children()
        .find(|child| child.tag_name().name() == name)
        .ok_or_else(|| Error::MissingElement(name.to_string()))
}

// -- Accessory parsing functions --
fn parse_points(spatial_element: &Node) -> Result<HashMap<String, Point>> {
    let mut points = HashMap::new();
    let gm_point_iter = spatial_element.children().filter(|child| {
        child.tag_name().name() == "GM_Point"
            && child.tag_name().namespace() == get_xml_namespace(Some("zmn"))
    });
    for point in gm_point_iter {
        let pos = point
            .descendants()
            .find(|child| {
                child.tag_name().name() == "DirectPosition"
                    && child.tag_name().namespace() == get_xml_namespace(Some("zmn"))
            })
            .ok_or_else(|| Error::MissingElement("pos".to_string()))?;
        let mut x: Option<f64> = None;
        let mut y: Option<f64> = None;
        for xy in pos.children() {
            if xy.tag_name().name() == "X" {
                x = Some(xy.text().unwrap_or("0").parse::<f64>()?);
            } else if xy.tag_name().name() == "Y" {
                y = Some(xy.text().unwrap_or("0").parse::<f64>()?);
            }
        }
        let x = x.ok_or_else(|| Error::MissingElement("X".to_string()))?;
        let y = y.ok_or_else(|| Error::MissingElement("Y".to_string()))?;
        let pos = Point::new(x, y);
        let point_id = point
            .attribute("id")
            .ok_or_else(|| Error::MissingAttribute {
                element: "GM_Point".to_string(),
                attribute: "id".to_string(),
            })?;
        points.insert(point_id.to_string(), pos);
    }
    Ok(points)
}

fn parse_curves(
    spatial_element: &Node,
    points: &HashMap<String, Point>,
) -> Result<HashMap<String, Curve>> {
    let mut curves = HashMap::new();
    let zmn_ns = get_xml_namespace(Some("zmn"));

    for curve in spatial_element.children().filter(|child| {
        child.tag_name().name() == "GM_Curve" && child.tag_name().namespace() == zmn_ns
    }) {
        let curve_id = curve
            .attribute("id")
            .ok_or_else(|| Error::MissingAttribute {
                element: "GM_Curve".to_string(),
                attribute: "id".to_string(),
            })?;

        let segment = curve
            .children()
            .find(|child| {
                child.tag_name().name() == "GM_Curve.segment"
                    && child.tag_name().namespace() == zmn_ns
            })
            .ok_or_else(|| Error::MissingElement("GM_Curve.segment".to_string()))?;

        let column = segment
            .descendants()
            .find(|child| {
                child.tag_name().name() == "GM_PointArray.column"
                    && child.tag_name().namespace() == zmn_ns
            })
            .ok_or_else(|| Error::MissingElement("GM_PointArray.column".to_string()))?;
        let pos = column
            .first_element_child()
            .ok_or_else(|| Error::MissingElement("GM_Position.*".to_string()))?;

        let (x, y) = if pos.tag_name().name() == "GM_Position.indirect" {
            let r#ref = pos
                .first_element_child()
                .ok_or_else(|| Error::MissingElement("GM_Position.indirect".to_string()))?;
            let idref = r#ref
                .attribute("idref")
                .ok_or_else(|| Error::MissingAttribute {
                    element: "GM_Position.indirect".to_string(),
                    attribute: "idref".to_string(),
                })?;
            let point = points
                .get(idref)
                .ok_or_else(|| Error::PointNotFound(idref.to_string()))?;

            (point.x(), point.y())
        } else if pos.tag_name().name() == "GM_Position.direct" {
            let x = pos
                .children()
                .find(|child| child.tag_name().name() == "X")
                .ok_or_else(|| Error::MissingElement("X".to_string()))?
                .text()
                .ok_or_else(|| Error::MissingElement("X".to_string()))?
                .parse::<f64>()?;
            let y = pos
                .children()
                .find(|child| child.tag_name().name() == "Y")
                .ok_or_else(|| Error::MissingElement("Y".to_string()))?
                .text()
                .ok_or_else(|| Error::MissingElement("Y".to_string()))?
                .parse::<f64>()?;
            (x, y)
        } else {
            return Err(Error::UnexpectedElement(pos.tag_name().name().to_string()));
        };

        let curve_point = Curve::new(y, x);
        curves.insert(curve_id.to_string(), curve_point);
    }

    Ok(curves)
}

fn transform_curves_crs(
    curves: &mut HashMap<String, Curve>,
    source_crs: &str,
    target_crs: &str,
) -> Result<()> {
    let transformer = Proj::new_known_crs(source_crs, target_crs, None)
        .map_err(|e| Error::Projection(e.to_string()))?;

    for curve in curves.values_mut() {
        let (new_x, new_y) = transformer
            .convert((curve.x(), curve.y()))
            .map_err(|e| Error::Projection(e.to_string()))?;
        *curve = Point::new(new_x, new_y);
    }

    Ok(())
}

fn parse_surfaces(
    spatial_element: &Node,
    curves: &HashMap<String, Curve>,
) -> Result<HashMap<String, Surface>> {
    let mut surfaces = HashMap::new();
    let zmn_ns = get_xml_namespace(Some("zmn"));

    for surface in spatial_element.children().filter(|child| {
        child.tag_name().name() == "GM_Surface" && child.tag_name().namespace() == zmn_ns
    }) {
        let polygons = surface
            .children()
            .filter(|child| {
                child.tag_name().name() == "GM_Surface.patch"
                    && child.tag_name().namespace() == zmn_ns
            })
            .flat_map(|patch| {
                patch.children().filter(|child| {
                    child.tag_name().name() == "GM_Polygon"
                        && child.tag_name().namespace() == zmn_ns
                })
            })
            .collect::<Vec<_>>();
        let polygon = polygons
            .first()
            .ok_or_else(|| Error::MissingElement("GM_Surface.patch".to_string()))?;
        let surface_id = surface
            .attribute("id")
            .ok_or_else(|| Error::MissingAttribute {
                element: "GM_Surface".to_string(),
                attribute: "id".to_string(),
            })?;

        let exterior = polygon
            .descendants()
            .find(|child| {
                child.tag_name().name() == "GM_SurfaceBoundary.exterior"
                    && child.tag_name().namespace() == zmn_ns
            })
            .ok_or_else(|| Error::MissingElement("GM_SurfaceBoundary.exterior".to_string()))?;

        let mut ring: Vec<Point> = Vec::new();
        for cc in exterior
            .descendants()
            .filter(|child| {
                child.tag_name().name() == "GM_Ring" && child.tag_name().namespace() == zmn_ns
            })
            .flat_map(|ring| ring.children().filter(|child| child.is_element()))
        {
            let curve_id = cc
                .attribute("idref")
                .ok_or_else(|| Error::MissingAttribute {
                    element: cc.tag_name().name().to_string(),
                    attribute: "idref".to_string(),
                })?;
            let curve = curves
                .get(curve_id)
                .ok_or_else(|| Error::PointNotFound(curve_id.to_string()))?;
            ring.push(*curve);
        }
        let exterior_ring = LineString::from(ring);

        let mut interior_rings: Vec<LineString> = Vec::new();
        for interior in polygon
            .descendants()
            .filter(|child| {
                child.tag_name().name() == "GM_SurfaceBoundary.interior"
                    && child.tag_name().namespace() == zmn_ns
            })
            .flat_map(|ring| ring.children().filter(|child| child.is_element()))
        {
            let mut ring: Vec<Point> = Vec::new();
            for cc in interior
                .descendants()
                .filter(|child| {
                    child.tag_name().name() == "GM_Ring" && child.tag_name().namespace() == zmn_ns
                })
                .flat_map(|ring| ring.children().filter(|child| child.is_element()))
            {
                let curve_id = cc
                    .attribute("idref")
                    .ok_or_else(|| Error::MissingAttribute {
                        element: cc.tag_name().name().to_string(),
                        attribute: "idref".to_string(),
                    })?;
                let curve = curves
                    .get(curve_id)
                    .ok_or_else(|| Error::PointNotFound(curve_id.to_string()))?;
                ring.push(*curve);
            }
            interior_rings.push(LineString::from(ring));
        }

        surfaces.insert(
            surface_id.to_string(),
            MultiPolygon::new(vec![Polygon::new(exterior_ring, interior_rings)]),
        );
    }

    Ok(surfaces)
}

fn parse_features(
    subject_elem: &Node,
    surfaces: &HashMap<String, Surface>,
    options: &ParseOptions,
) -> Result<Vec<Feature>> {
    let mut features: Vec<Feature> = Vec::new();
    for fude in subject_elem.children().filter(|child| {
        child.tag_name().name() == "筆" && child.tag_name().namespace() == get_xml_namespace(None)
    }) {
        let fude_id = fude
            .attribute("id")
            .ok_or_else(|| Error::MissingAttribute {
                element: "筆".to_string(),
                attribute: "id".to_string(),
            })?;

        let mut geometry: Option<MultiPolygon> = None;
        let mut prop_map: HashMap<String, String> = HashMap::new();
        for entry in fude.children().filter(|child| child.is_element()) {
            let name = entry.tag_name().name();
            if name == "形状" {
                let idref = entry
                    .attribute("idref")
                    .ok_or_else(|| Error::MissingAttribute {
                        element: "形状".to_string(),
                        attribute: "idref".to_string(),
                    })?;
                geometry = surfaces.get(idref).cloned();
            } else {
                let value = entry.text().unwrap_or("").to_string();
                prop_map.insert(name.to_string(), value);
            }
        }

        if !options.include_chikugai {
            let chiban = prop_map
                .get("地番")
                .ok_or_else(|| Error::MissingElement("地番".to_string()))?;
            if chiban.contains("地区外") || chiban.contains("別図") {
                continue;
            }
        }

        features.push(Feature {
            geometry: geometry.ok_or_else(|| Error::MissingElement("geometry".to_string()))?,
            props: FeatureProperties {
                筆id: fude_id.to_string(),
                精度区分: prop_map.remove("精度区分"),
                大字コード: prop_map.remove("大字コード"),
                丁目コード: prop_map.remove("丁目コード"),
                小字コード: prop_map.remove("小字コード"),
                予備コード: prop_map.remove("予備コード"),
                大字名: prop_map.remove("大字名"),
                丁目名: prop_map.remove("丁目名"),
                小字名: prop_map.remove("小字名"),
                予備名: prop_map.remove("予備名"),
                地番: prop_map.remove("地番"),
                座標値種別: prop_map.remove("座標値種別"),
                筆界未定構成筆: prop_map.remove("筆界未定構成筆"),
            },
        });
    }
    Ok(features)
}

fn parse_base_properties(root: &Node) -> Result<CommonProperties> {
    let map_name = get_child_element(root, "地図名")?
        .text()
        .ok_or_else(|| Error::MissingElement("地図名".to_string()))?;
    let city_code = get_child_element(root, "市区町村コード")?
        .text()
        .ok_or_else(|| Error::MissingElement("市区町村コード".to_string()))?;
    let city_name = get_child_element(root, "市区町村名")?
        .text()
        .ok_or_else(|| Error::MissingElement("市区町村名".to_string()))?;
    let crs = get_child_element(root, "座標系")?
        .text()
        .ok_or_else(|| Error::MissingElement("座標系".to_string()))?;
    let crs_det_elem = get_child_element(root, "測地系判別").ok();
    let crs_det = if let Some(crs_det_elem) = crs_det_elem {
        Some(crs_det_elem.text().unwrap().to_string())
    } else {
        None
    };

    Ok(CommonProperties {
        地図名: map_name.to_string(),
        市区町村コード: city_code.to_string(),
        市区町村名: city_name.to_string(),
        座標系: crs.to_string(),
        測地系判別: crs_det,
    })
}

pub struct ParsedXML {
    pub file_name: String,
    pub features: Vec<Feature>,
    pub common_props: CommonProperties,
}

// --- Main Parsing Function ---
pub fn parse_xml_content(file: &FileData, options: &ParseOptions) -> Result<ParsedXML> {
    let xml_bytes = &file.contents;
    let xml_text = std::str::from_utf8(xml_bytes)?;
    let doc = Document::parse(xml_text)?;
    let root = doc.root_element();

    let common_props = parse_base_properties(&root)?;

    let crs_string = get_child_element(&root, "座標系")?
        .text()
        .ok_or_else(|| Error::MissingElement("座標系".to_string()))?;
    let crs = get_epsg_code(crs_string)?;
    if crs.is_none() && !options.include_arbitrary_crs {
        return Ok(ParsedXML {
            file_name: file.file_name.clone(),
            features: vec![],
            common_props,
        });
    }

    let spatial_element = get_child_element(&root, "空間属性")?;
    let points = parse_points(&spatial_element)?;
    let mut curves = parse_curves(&spatial_element, &points)?;
    if let Some(crs) = crs {
        transform_curves_crs(&mut curves, crs, TARGET_CRS)?;
    }

    let surfaces = parse_surfaces(&spatial_element, &curves)?;
    let subject_elem = get_child_element(&root, "主題属性")?;

    let features = parse_features(&subject_elem, &surfaces, options)?;
    Ok(ParsedXML {
        file_name: file.file_name.clone(),
        features,
        common_props,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_parse_xml_content() {
        // Construct the path relative to the Cargo manifest directory
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let xml_path = Path::new(&manifest_dir).join("testdata/46505-3411-56.xml");
        let xml_bytes =
            fs::read(&xml_path).expect(&format!("Failed to read XML file: {:?}", xml_path));
        let options = ParseOptions {
            include_arbitrary_crs: true,
            include_chikugai: true,
        };
        let ParsedXML {
            file_name: _,
            features,
            common_props,
        } = parse_xml_content(
            &FileData {
                file_name: "46505-3411-56.xml".to_string(),
                contents: xml_bytes,
            },
            &options,
        )
        .expect("Failed to parse XML");
        assert_eq!(common_props.地図名, "AYA1anbou22B04_2000");
        assert_eq!(common_props.市区町村コード, "46505");
        assert_eq!(common_props.市区町村名, "熊毛郡屋久島町");

        assert_eq!(features.len(), 2994);
        let feature = &features[0];
        assert_eq!(feature.props.筆id, "H000000001");
        assert_eq!(feature.props.地番, Some("1".to_string()));
    }
}

use crate::constants::get_proj;
use crate::error::{Error, Result};
use crate::types::{CommonProperties, Feature, FeatureProperties};
use crate::{ParsedXML, 筆界未定構成筆};
use geo::algorithm::interior_point::InteriorPoint;
use geo_types::{LineString, Point, Polygon};
use proj4rs::proj::Proj;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use rustc_hash::FxHashMap as HashMap;

// --- Type Aliases ---
type Curve = Point;
type Surface = Polygon;

#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    pub include_arbitrary_crs: bool,
    pub include_chikugai: bool,
}

#[derive(Default)]
struct CommonPropsBuilder {
    map_name: Option<String>,
    city_code: Option<String>,
    city_name: Option<String>,
    crs: Option<String>,
    crs_det: Option<String>,
}

impl CommonPropsBuilder {
    fn into_common_props(self) -> Result<CommonProperties> {
        Ok(CommonProperties {
            地図名: self
                .map_name
                .ok_or_else(|| Error::MissingElement("地図名".to_string()))?,
            市区町村コード: self
                .city_code
                .ok_or_else(|| Error::MissingElement("市区町村コード".to_string()))?,
            市区町村名: self
                .city_name
                .ok_or_else(|| Error::MissingElement("市区町村名".to_string()))?,
            座標系: self
                .crs
                .ok_or_else(|| Error::MissingElement("座標系".to_string()))?,
            測地系判別: self.crs_det,
        })
    }
}

#[derive(Default)]
struct PointBuilder {
    id: String,
    saw_position: bool,
    saw_direct_position: bool,
    x: Option<f64>,
    y: Option<f64>,
}

#[derive(Clone, Copy)]
enum CurvePositionKind {
    Direct,
    Indirect,
}

#[derive(Default)]
struct CurveBuilder {
    id: String,
    saw_segment: bool,
    saw_first_column: bool,
    first_column_depth: Option<usize>,
    position_kind: Option<CurvePositionKind>,
    position_depth: Option<usize>,
    direct_x: Option<f64>,
    direct_y: Option<f64>,
    indirect_ref: Option<String>,
}

#[derive(Clone, Copy)]
enum BoundaryKind {
    Exterior,
    Interior(usize),
}

#[derive(Default)]
struct SurfaceBuilder {
    id: String,
    saw_patch: bool,
    saw_polygon: bool,
    saw_polygon_boundary: bool,
    saw_surface_boundary: bool,
    saw_exterior: bool,
    active_boundary: Option<BoundaryKind>,
    boundary_depth: Option<usize>,
    ring_depth: Option<usize>,
    exterior_points: Vec<Point>,
    interior_points: Vec<Vec<Point>>,
}

#[derive(Default)]
struct FudeBuilder {
    id: String,
    geometry_ref: Option<String>,
    precision_class: Option<String>,
    ooaza_code: Option<String>,
    chome_code: Option<String>,
    koaza_code: Option<String>,
    yobi_code: Option<String>,
    ooaza_name: Option<String>,
    chome_name: Option<String>,
    koaza_name: Option<String>,
    yobi_name: Option<String>,
    parcel_number: Option<String>,
    coordinate_value_type: Option<String>,
    constituents: Vec<筆界未定構成筆>,
}

#[derive(Clone, Copy)]
enum TextTarget {
    RootMapName,
    RootCityCode,
    RootCityName,
    RootCrs,
    RootCrsDet,
    PointX,
    PointY,
    CurveDirectX,
    CurveDirectY,
    FudePrecisionClass,
    FudeOoazaCode,
    FudeChomeCode,
    FudeKoazaCode,
    FudeYobiCode,
    FudeOoazaName,
    FudeChomeName,
    FudeKoazaName,
    FudeYobiName,
    FudeParcelNumber,
    FudeCoordinateValueType,
    ConstituentOoazaCode,
    ConstituentChomeCode,
    ConstituentKoazaCode,
    ConstituentYobiCode,
    ConstituentOoazaName,
    ConstituentChomeName,
    ConstituentKoazaName,
    ConstituentYobiName,
    ConstituentParcelNumber,
}

struct ActiveText {
    target: TextTarget,
    depth: usize,
    value: String,
    has_text: bool,
}

impl ActiveText {
    fn new(target: TextTarget, depth: usize) -> Self {
        Self {
            target,
            depth,
            value: String::new(),
            has_text: false,
        }
    }
}

struct StreamParser<'a> {
    options: &'a ParseOptions,
    common_builder: CommonPropsBuilder,
    common_props: Option<CommonProperties>,
    common_checked: bool,
    skip_features: bool,
    source_crs: Option<&'static Proj>,
    target_crs: Option<&'static Proj>,

    in_spatial: bool,
    in_subject: bool,
    saw_spatial: bool,
    saw_subject: bool,

    points: HashMap<String, Point>,
    curves: HashMap<String, Curve>,
    surfaces: HashMap<String, Surface>,
    features: Vec<Feature>,

    point: Option<PointBuilder>,
    curve: Option<CurveBuilder>,
    surface: Option<SurfaceBuilder>,
    fude: Option<FudeBuilder>,
    constituent: Option<筆界未定構成筆>,

    active_text: Option<ActiveText>,
}

impl<'a> StreamParser<'a> {
    fn new(options: &'a ParseOptions) -> Self {
        Self {
            options,
            common_builder: CommonPropsBuilder::default(),
            common_props: None,
            common_checked: false,
            skip_features: false,
            source_crs: None,
            target_crs: None,
            in_spatial: false,
            in_subject: false,
            saw_spatial: false,
            saw_subject: false,
            points: HashMap::with_capacity_and_hasher(16_384, Default::default()),
            curves: HashMap::with_capacity_and_hasher(32_768, Default::default()),
            surfaces: HashMap::with_capacity_and_hasher(4_096, Default::default()),
            features: Vec::with_capacity(4_096),
            point: None,
            curve: None,
            surface: None,
            fude: None,
            constituent: None,
            active_text: None,
        }
    }

    fn ensure_common_and_crs(&mut self) -> Result<()> {
        if self.common_checked {
            return Ok(());
        }

        let common_props = std::mem::take(&mut self.common_builder).into_common_props()?;
        let source_crs = get_proj(&common_props.座標系)?;

        self.common_props = Some(common_props);
        self.common_checked = true;

        match source_crs {
            Some(source_crs) => {
                let target_crs = get_proj("WGS84")?.expect("WGS84 CRS not found");
                self.source_crs = Some(source_crs);
                self.target_crs = Some(target_crs);
            }
            None => {
                if !self.options.include_arbitrary_crs {
                    self.skip_features = true;
                }
            }
        }

        Ok(())
    }

    fn finish(mut self, file_name: String) -> Result<ParsedXML> {
        if !self.common_checked {
            self.ensure_common_and_crs()?;
        }

        let common_props = self
            .common_props
            .ok_or_else(|| Error::MissingElement("地図名".to_string()))?;

        if self.skip_features {
            return Ok(ParsedXML {
                file_name,
                features: Vec::new(),
                common_props,
            });
        }

        if !self.saw_spatial {
            return Err(Error::MissingElement("空間属性".to_string()));
        }

        if !self.saw_subject {
            return Err(Error::MissingElement("主題属性".to_string()));
        }

        Ok(ParsedXML {
            file_name,
            features: self.features,
            common_props,
        })
    }

    fn begin_text(&mut self, target: TextTarget, depth: usize) {
        self.active_text = Some(ActiveText::new(target, depth));
    }

    fn push_text_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(active) = &mut self.active_text {
            active.value.push_str(std::str::from_utf8(bytes)?);
            active.has_text = true;
        }
        Ok(())
    }

    fn finalize_text_if_needed(&mut self, depth: usize) -> Result<()> {
        let should_finalize = match &self.active_text {
            Some(active) => active.depth == depth,
            None => false,
        };

        if !should_finalize {
            return Ok(());
        }

        let active = self
            .active_text
            .take()
            .expect("active text must be present");
        let value = if active.has_text {
            Some(active.value.as_str())
        } else {
            None
        };
        self.apply_text(active.target, value)
    }

    fn apply_text(&mut self, target: TextTarget, value: Option<&str>) -> Result<()> {
        match target {
            TextTarget::RootMapName => self.common_builder.map_name = value.map(str::to_owned),
            TextTarget::RootCityCode => self.common_builder.city_code = value.map(str::to_owned),
            TextTarget::RootCityName => self.common_builder.city_name = value.map(str::to_owned),
            TextTarget::RootCrs => self.common_builder.crs = value.map(str::to_owned),
            TextTarget::RootCrsDet => self.common_builder.crs_det = value.map(str::to_owned),

            TextTarget::PointX => {
                if let Some(point) = &mut self.point {
                    point.x = value.map(|v| v.parse()).transpose()?;
                }
            }
            TextTarget::PointY => {
                if let Some(point) = &mut self.point {
                    point.y = value.map(|v| v.parse()).transpose()?;
                }
            }

            TextTarget::CurveDirectX => {
                if let Some(curve) = &mut self.curve {
                    curve.direct_x = value.map(|v| v.parse()).transpose()?;
                }
            }
            TextTarget::CurveDirectY => {
                if let Some(curve) = &mut self.curve {
                    curve.direct_y = value.map(|v| v.parse()).transpose()?;
                }
            }

            TextTarget::FudePrecisionClass => {
                if let Some(fude) = &mut self.fude {
                    fude.precision_class = value.map(str::to_owned);
                }
            }
            TextTarget::FudeOoazaCode => {
                if let Some(fude) = &mut self.fude {
                    fude.ooaza_code = Some(value.unwrap_or("").to_owned());
                }
            }
            TextTarget::FudeChomeCode => {
                if let Some(fude) = &mut self.fude {
                    fude.chome_code = Some(value.unwrap_or("").to_owned());
                }
            }
            TextTarget::FudeKoazaCode => {
                if let Some(fude) = &mut self.fude {
                    fude.koaza_code = Some(value.unwrap_or("").to_owned());
                }
            }
            TextTarget::FudeYobiCode => {
                if let Some(fude) = &mut self.fude {
                    fude.yobi_code = Some(value.unwrap_or("").to_owned());
                }
            }
            TextTarget::FudeOoazaName => {
                if let Some(fude) = &mut self.fude {
                    fude.ooaza_name = value.map(str::to_owned);
                }
            }
            TextTarget::FudeChomeName => {
                if let Some(fude) = &mut self.fude {
                    fude.chome_name = value.map(str::to_owned);
                }
            }
            TextTarget::FudeKoazaName => {
                if let Some(fude) = &mut self.fude {
                    fude.koaza_name = value.map(str::to_owned);
                }
            }
            TextTarget::FudeYobiName => {
                if let Some(fude) = &mut self.fude {
                    fude.yobi_name = value.map(str::to_owned);
                }
            }
            TextTarget::FudeParcelNumber => {
                if let Some(fude) = &mut self.fude {
                    fude.parcel_number = Some(value.unwrap_or("").to_owned());
                }
            }
            TextTarget::FudeCoordinateValueType => {
                if let Some(fude) = &mut self.fude {
                    fude.coordinate_value_type = value.map(str::to_owned);
                }
            }

            TextTarget::ConstituentOoazaCode => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.大字コード = value.unwrap_or("").to_owned();
                }
            }
            TextTarget::ConstituentChomeCode => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.丁目コード = value.unwrap_or("").to_owned();
                }
            }
            TextTarget::ConstituentKoazaCode => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.小字コード = value.unwrap_or("").to_owned();
                }
            }
            TextTarget::ConstituentYobiCode => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.予備コード = value.unwrap_or("").to_owned();
                }
            }
            TextTarget::ConstituentOoazaName => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.大字名 = value.map(str::to_owned);
                }
            }
            TextTarget::ConstituentChomeName => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.丁目名 = value.map(str::to_owned);
                }
            }
            TextTarget::ConstituentKoazaName => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.小字名 = value.map(str::to_owned);
                }
            }
            TextTarget::ConstituentYobiName => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.予備名 = value.map(str::to_owned);
                }
            }
            TextTarget::ConstituentParcelNumber => {
                if let Some(constituent) = &mut self.constituent {
                    constituent.地番 = value.unwrap_or("").to_owned();
                }
            }
        }
        Ok(())
    }

    fn handle_start(&mut self, start: &BytesStart<'_>, depth: usize) -> Result<()> {
        let start_name = start.name();
        let name = local_name(start_name.as_ref());

        if depth == 2 {
            match () {
                _ if name_eq(name, "地図名") => self.begin_text(TextTarget::RootMapName, depth),
                _ if name_eq(name, "市区町村コード") => {
                    self.begin_text(TextTarget::RootCityCode, depth)
                }
                _ if name_eq(name, "市区町村名") => {
                    self.begin_text(TextTarget::RootCityName, depth)
                }
                _ if name_eq(name, "座標系") => self.begin_text(TextTarget::RootCrs, depth),
                _ if name_eq(name, "測地系判別") => {
                    self.begin_text(TextTarget::RootCrsDet, depth)
                }
                _ if name_eq(name, "空間属性") => {
                    self.ensure_common_and_crs()?;
                    if !self.skip_features {
                        self.saw_spatial = true;
                        self.in_spatial = true;
                    }
                }
                _ if name_eq(name, "主題属性") => {
                    self.ensure_common_and_crs()?;
                    if !self.skip_features {
                        self.saw_subject = true;
                        self.in_subject = true;
                    }
                }
                _ => {}
            }
        }

        if self.skip_features {
            return Ok(());
        }

        if self.in_spatial {
            self.handle_spatial_start(start, name, depth)?;
        }

        if self.in_subject {
            self.handle_subject_start(start, name, depth)?;
        }

        Ok(())
    }

    fn handle_spatial_start(
        &mut self,
        start: &BytesStart<'_>,
        name: &[u8],
        depth: usize,
    ) -> Result<()> {
        if depth == 3 && name == b"GM_Point" {
            self.start_point(start)?;
            return Ok(());
        }

        if depth == 3 && name == b"GM_Curve" {
            self.start_curve(start)?;
            return Ok(());
        }

        if depth == 3 && name == b"GM_Surface" {
            self.start_surface(start)?;
            return Ok(());
        }

        if let Some(point) = &mut self.point {
            match name {
                b"GM_Point.position" => point.saw_position = true,
                b"DirectPosition" => point.saw_direct_position = true,
                b"X" => self.begin_text(TextTarget::PointX, depth),
                b"Y" => self.begin_text(TextTarget::PointY, depth),
                _ => {}
            }
        }

        if let Some(curve) = &mut self.curve {
            if name == b"GM_Curve.segment" {
                curve.saw_segment = true;
            }

            if name == b"GM_PointArray.column" && !curve.saw_first_column {
                curve.saw_first_column = true;
                curve.first_column_depth = Some(depth);
            }

            if let Some(column_depth) = curve.first_column_depth
                && curve.position_kind.is_none()
                && depth == column_depth + 1
            {
                match name {
                    b"GM_Position.indirect" => {
                        curve.position_kind = Some(CurvePositionKind::Indirect);
                        curve.position_depth = Some(depth);
                    }
                    b"GM_Position.direct" => {
                        curve.position_kind = Some(CurvePositionKind::Direct);
                        curve.position_depth = Some(depth);
                    }
                    _ => {
                        return Err(Error::UnexpectedElement(name_to_string(name)));
                    }
                }
            }

            if let (Some(position_kind), Some(position_depth)) =
                (curve.position_kind, curve.position_depth)
            {
                match position_kind {
                    CurvePositionKind::Direct => {
                        if depth == position_depth + 1 {
                            match name {
                                b"X" => self.begin_text(TextTarget::CurveDirectX, depth),
                                b"Y" => self.begin_text(TextTarget::CurveDirectY, depth),
                                _ => {}
                            }
                        }
                    }
                    CurvePositionKind::Indirect => {
                        if depth == position_depth + 1 && curve.indirect_ref.is_none() {
                            curve.indirect_ref = Some(required_attribute(start, "idref")?);
                        }
                    }
                }
            }
        }

        if let Some(surface) = &mut self.surface {
            match name {
                b"GM_Surface.patch" => surface.saw_patch = true,
                b"GM_Polygon" => surface.saw_polygon = true,
                b"GM_Polygon.boundary" => surface.saw_polygon_boundary = true,
                b"GM_SurfaceBoundary" => surface.saw_surface_boundary = true,
                b"GM_SurfaceBoundary.exterior" => {
                    surface.saw_exterior = true;
                    surface.active_boundary = Some(BoundaryKind::Exterior);
                    surface.boundary_depth = Some(depth);
                }
                b"GM_SurfaceBoundary.interior" => {
                    let interior_index = surface.interior_points.len();
                    surface.interior_points.push(Vec::new());
                    surface.active_boundary = Some(BoundaryKind::Interior(interior_index));
                    surface.boundary_depth = Some(depth);
                }
                b"GM_Ring" => {
                    if surface.active_boundary.is_some() {
                        surface.ring_depth = Some(depth);
                    }
                }
                _ => {}
            }

            if let Some(ring_depth) = surface.ring_depth
                && depth == ring_depth + 1
            {
                let idref = required_attribute(start, "idref")?;
                let curve = *self
                    .curves
                    .get(&idref)
                    .ok_or_else(|| Error::PointNotFound(idref.clone()))?;

                match surface.active_boundary {
                    Some(BoundaryKind::Exterior) => surface.exterior_points.push(curve),
                    Some(BoundaryKind::Interior(idx)) => {
                        if let Some(interior) = surface.interior_points.get_mut(idx) {
                            interior.push(curve);
                        }
                    }
                    None => {}
                }
            }
        }

        Ok(())
    }

    fn handle_subject_start(
        &mut self,
        start: &BytesStart<'_>,
        name: &[u8],
        depth: usize,
    ) -> Result<()> {
        if depth == 3 && name_eq(name, "筆") {
            self.start_fude(start)?;
            return Ok(());
        }

        if self.fude.is_none() {
            return Ok(());
        }

        if name_eq(name, "筆界未定構成筆") {
            self.constituent = Some(筆界未定構成筆::default());
            return Ok(());
        }

        if self.constituent.is_some() {
            match () {
                _ if name_eq(name, "大字コード") => {
                    self.begin_text(TextTarget::ConstituentOoazaCode, depth)
                }
                _ if name_eq(name, "丁目コード") => {
                    self.begin_text(TextTarget::ConstituentChomeCode, depth)
                }
                _ if name_eq(name, "小字コード") => {
                    self.begin_text(TextTarget::ConstituentKoazaCode, depth)
                }
                _ if name_eq(name, "予備コード") => {
                    self.begin_text(TextTarget::ConstituentYobiCode, depth)
                }
                _ if name_eq(name, "大字名") => {
                    self.begin_text(TextTarget::ConstituentOoazaName, depth)
                }
                _ if name_eq(name, "丁目名") => {
                    self.begin_text(TextTarget::ConstituentChomeName, depth)
                }
                _ if name_eq(name, "小字名") => {
                    self.begin_text(TextTarget::ConstituentKoazaName, depth)
                }
                _ if name_eq(name, "予備名") => {
                    self.begin_text(TextTarget::ConstituentYobiName, depth)
                }
                _ if name_eq(name, "地番") => {
                    self.begin_text(TextTarget::ConstituentParcelNumber, depth)
                }
                _ => {}
            }
            return Ok(());
        }

        match () {
            _ if name_eq(name, "形状") => {
                if let Some(fude) = &mut self.fude {
                    fude.geometry_ref = Some(required_attribute(start, "idref")?);
                }
            }
            _ if name_eq(name, "精度区分") => {
                self.begin_text(TextTarget::FudePrecisionClass, depth)
            }
            _ if name_eq(name, "大字コード") => {
                self.begin_text(TextTarget::FudeOoazaCode, depth)
            }
            _ if name_eq(name, "丁目コード") => {
                self.begin_text(TextTarget::FudeChomeCode, depth)
            }
            _ if name_eq(name, "小字コード") => {
                self.begin_text(TextTarget::FudeKoazaCode, depth)
            }
            _ if name_eq(name, "予備コード") => {
                self.begin_text(TextTarget::FudeYobiCode, depth)
            }
            _ if name_eq(name, "大字名") => self.begin_text(TextTarget::FudeOoazaName, depth),
            _ if name_eq(name, "丁目名") => self.begin_text(TextTarget::FudeChomeName, depth),
            _ if name_eq(name, "小字名") => self.begin_text(TextTarget::FudeKoazaName, depth),
            _ if name_eq(name, "予備名") => self.begin_text(TextTarget::FudeYobiName, depth),
            _ if name_eq(name, "地番") => self.begin_text(TextTarget::FudeParcelNumber, depth),
            _ if name_eq(name, "座標値種別") => {
                self.begin_text(TextTarget::FudeCoordinateValueType, depth)
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_end(&mut self, name: &[u8], depth: usize) -> Result<()> {
        self.finalize_text_if_needed(depth)?;

        if self.skip_features {
            return Ok(());
        }

        if self.in_spatial {
            self.handle_spatial_end(name, depth)?;
        }

        if self.in_subject {
            self.handle_subject_end(name)?;
        }

        if depth == 2 {
            match () {
                _ if name_eq(name, "空間属性") => self.in_spatial = false,
                _ if name_eq(name, "主題属性") => self.in_subject = false,
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_spatial_end(&mut self, name: &[u8], depth: usize) -> Result<()> {
        if let Some(curve) = &mut self.curve
            && curve.position_depth == Some(depth)
            && (name == b"GM_Position.direct" || name == b"GM_Position.indirect")
        {
            curve.position_depth = None;
        }

        if let Some(surface) = &mut self.surface {
            if surface.ring_depth == Some(depth) && name == b"GM_Ring" {
                surface.ring_depth = None;
            }

            if surface.boundary_depth == Some(depth)
                && (name == b"GM_SurfaceBoundary.exterior"
                    || name == b"GM_SurfaceBoundary.interior")
            {
                surface.active_boundary = None;
                surface.boundary_depth = None;
            }
        }

        match name {
            b"GM_Point" => self.finish_point()?,
            b"GM_Curve" => self.finish_curve()?,
            b"GM_Surface" => self.finish_surface()?,
            _ => {}
        }

        Ok(())
    }

    fn handle_subject_end(&mut self, name: &[u8]) -> Result<()> {
        match () {
            _ if name_eq(name, "筆界未定構成筆") => {
                if let Some(constituent) = self.constituent.take()
                    && let Some(fude) = &mut self.fude
                {
                    fude.constituents.push(constituent);
                }
            }
            _ if name_eq(name, "筆") => self.finish_fude()?,
            _ => {}
        }

        Ok(())
    }

    fn start_point(&mut self, start: &BytesStart<'_>) -> Result<()> {
        self.point = Some(PointBuilder {
            id: required_attribute(start, "id")?,
            ..Default::default()
        });
        Ok(())
    }

    fn start_curve(&mut self, start: &BytesStart<'_>) -> Result<()> {
        self.curve = Some(CurveBuilder {
            id: required_attribute(start, "id")?,
            ..Default::default()
        });
        Ok(())
    }

    fn start_surface(&mut self, start: &BytesStart<'_>) -> Result<()> {
        self.surface = Some(SurfaceBuilder {
            id: required_attribute(start, "id")?,
            ..Default::default()
        });
        Ok(())
    }

    fn start_fude(&mut self, start: &BytesStart<'_>) -> Result<()> {
        self.fude = Some(FudeBuilder {
            id: required_attribute(start, "id")?,
            ..Default::default()
        });
        Ok(())
    }

    fn finish_point(&mut self) -> Result<()> {
        let point = match self.point.take() {
            Some(point) => point,
            None => return Ok(()),
        };

        if !point.saw_position {
            return Err(Error::MissingElement("GM_Point.position".to_string()));
        }

        if !point.saw_direct_position {
            return Err(Error::MissingElement("DirectPosition".to_string()));
        }

        let x = point
            .x
            .ok_or_else(|| Error::MissingElement("X".to_string()))?;
        let y = point
            .y
            .ok_or_else(|| Error::MissingElement("Y".to_string()))?;

        self.points.insert(point.id, Point::new(x, y));
        Ok(())
    }

    fn finish_curve(&mut self) -> Result<()> {
        let curve = match self.curve.take() {
            Some(curve) => curve,
            None => return Ok(()),
        };

        if !curve.saw_segment {
            return Err(Error::MissingElement("GM_Curve.segment".to_string()));
        }

        if !curve.saw_first_column {
            return Err(Error::MissingElement("GM_PointArray.column".to_string()));
        }

        let (x, y) = match curve.position_kind {
            Some(CurvePositionKind::Direct) => {
                let x = curve
                    .direct_x
                    .ok_or_else(|| Error::MissingElement("X".to_string()))?;
                let y = curve
                    .direct_y
                    .ok_or_else(|| Error::MissingElement("Y".to_string()))?;
                (x, y)
            }
            Some(CurvePositionKind::Indirect) => {
                let idref = curve
                    .indirect_ref
                    .ok_or_else(|| Error::MissingElement("GM_Position.indirect".to_string()))?;
                let point = self
                    .points
                    .get(&idref)
                    .ok_or_else(|| Error::PointNotFound(idref.clone()))?;
                (point.x(), point.y())
            }
            None => {
                return Err(Error::MissingElement("GM_Position.*".to_string()));
            }
        };

        let mut curve_point = Curve::new(y, x);
        if let (Some(source_crs), Some(target_crs)) = (self.source_crs, self.target_crs) {
            transform_curve_crs(&mut curve_point, source_crs, target_crs)?;
        }

        self.curves.insert(curve.id, curve_point);
        Ok(())
    }

    fn finish_surface(&mut self) -> Result<()> {
        let surface = match self.surface.take() {
            Some(surface) => surface,
            None => return Ok(()),
        };

        if !surface.saw_patch || !surface.saw_polygon {
            return Err(Error::MissingElement("GM_Surface.patch".to_string()));
        }

        if !surface.saw_polygon_boundary || !surface.saw_surface_boundary {
            return Err(Error::MissingElement("GM_SurfaceBoundary".to_string()));
        }

        if !surface.saw_exterior {
            return Err(Error::MissingElement(
                "GM_SurfaceBoundary.exterior".to_string(),
            ));
        }

        let exterior_ring = LineString::from(surface.exterior_points);
        let interior_rings = surface
            .interior_points
            .into_iter()
            .map(LineString::from)
            .collect::<Vec<_>>();

        self.surfaces
            .insert(surface.id, Polygon::new(exterior_ring, interior_rings));

        Ok(())
    }

    fn finish_fude(&mut self) -> Result<()> {
        let fude = match self.fude.take() {
            Some(fude) => fude,
            None => return Ok(()),
        };

        if !self.options.include_chikugai {
            match fude.parcel_number.as_ref() {
                Some(value) if value.contains("地区外") || value.contains("別図") => {
                    return Ok(());
                }
                Some(_) => {}
                None => return Err(Error::MissingElement("地番".to_string())),
            }
        }

        let geometry = fude
            .geometry_ref
            .as_ref()
            .and_then(|idref| self.surfaces.get(idref))
            .cloned()
            .ok_or_else(|| Error::MissingElement("geometry".to_string()))?;

        let ooaza_code = fude
            .ooaza_code
            .ok_or_else(|| Error::MissingElement("大字コード".to_string()))?;
        let chome_code = fude
            .chome_code
            .ok_or_else(|| Error::MissingElement("丁目コード".to_string()))?;
        let koaza_code = fude
            .koaza_code
            .ok_or_else(|| Error::MissingElement("小字コード".to_string()))?;
        let yobi_code = fude
            .yobi_code
            .ok_or_else(|| Error::MissingElement("予備コード".to_string()))?;
        let parcel_number = fude
            .parcel_number
            .ok_or_else(|| Error::MissingElement("地番".to_string()))?;

        let pop = point_on_polygon(&geometry)?;

        self.features.push(Feature {
            geometry,
            props: FeatureProperties {
                筆id: fude.id,
                精度区分: fude.precision_class,
                大字コード: ooaza_code,
                丁目コード: chome_code,
                小字コード: koaza_code,
                予備コード: yobi_code,
                大字名: fude.ooaza_name,
                丁目名: fude.chome_name,
                小字名: fude.koaza_name,
                予備名: fude.yobi_name,
                地番: parcel_number,
                座標値種別: fude.coordinate_value_type,
                筆界未定構成筆: fude.constituents,
                代表点緯度: pop.y(),
                代表点経度: pop.x(),
            },
        });

        Ok(())
    }
}

#[inline]
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(pos) => &name[pos + 1..],
        None => name,
    }
}

#[inline]
fn name_to_string(name: &[u8]) -> String {
    String::from_utf8_lossy(local_name(name)).into_owned()
}

#[inline]
fn name_eq(name: &[u8], expected: &str) -> bool {
    name == expected.as_bytes()
}

fn required_attribute(start: &BytesStart<'_>, attr_name: &str) -> Result<String> {
    for attr in start.attributes().with_checks(false).flatten() {
        if local_name(attr.key.as_ref()) == attr_name.as_bytes() {
            return Ok(std::str::from_utf8(attr.value.as_ref())?.to_owned());
        }
    }

    Err(Error::MissingAttribute {
        element: name_to_string(start.name().as_ref()),
        attribute: attr_name.to_string(),
    })
}

fn point_on_polygon(polygon: &Polygon) -> Result<Point<f64>> {
    // interior_point returns None if the polygon is empty or has no interior point
    // We've tested on 2024 data, and all polygons have an interior point
    polygon
        .interior_point()
        .ok_or(Error::InteriorPointUnavailable)
}

#[inline]
fn transform_curve_crs(curve: &mut Curve, source_crs: &Proj, target_crs: &Proj) -> Result<()> {
    let mut point = curve.x_y();
    proj4rs::transform::transform(source_crs, target_crs, &mut point)?;
    *curve = Point::new(point.0.to_degrees(), point.1.to_degrees());
    Ok(())
}

/// Transform all curves' coordinates from source_crs to target_crs in-place.
#[cfg(test)]
fn transform_curves_crs(
    curves: &mut HashMap<&str, Curve>,
    source_crs: &Proj,
    target_crs: &Proj,
) -> Result<()> {
    if curves.is_empty() {
        return Ok(());
    }

    for curve in curves.values_mut() {
        transform_curve_crs(curve, source_crs, target_crs)?;
    }

    Ok(())
}

// --- Main Parsing Function ---
pub fn parse_xml_content(
    file_name: &str,
    file_data: &str,
    options: &ParseOptions,
) -> Result<ParsedXML> {
    let file_name = file_name.to_string();
    let mut parser = StreamParser::new(options);
    let mut reader = Reader::from_str(file_data);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut depth = 0usize;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(start) => {
                parser.handle_start(&start, depth + 1)?;
                if parser.skip_features {
                    break;
                }
                depth += 1;
            }
            Event::Empty(start) => {
                parser.handle_start(&start, depth + 1)?;
                if parser.skip_features {
                    break;
                }

                let start_name = start.name();
                let name = local_name(start_name.as_ref());
                parser.handle_end(name, depth + 1)?;
                if parser.skip_features {
                    break;
                }
            }
            Event::End(end) => {
                parser.handle_end(local_name(end.name().as_ref()), depth)?;
                depth = depth.saturating_sub(1);
            }
            Event::Text(text) => parser.push_text_bytes(text.as_ref())?,
            Event::CData(text) => parser.push_text_bytes(text.as_ref())?,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    parser.finish(file_name)
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

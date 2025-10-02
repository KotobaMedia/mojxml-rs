use geo_types::Polygon;
#[cfg(feature = "geoparquet")]
use geoparquet_batch_writer::GeoParquetRowStruct;
use serde::Serialize;

pub struct ParsedXML {
    pub file_name: String,
    pub features: Vec<Feature>,
    pub common_props: CommonProperties,
}

#[derive(Debug, Clone)]
pub struct Feature {
    pub geometry: Polygon,
    pub props: FeatureProperties,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FeatureProperties {
    pub 筆id: String,
    pub 精度区分: Option<String>,
    pub 大字コード: String,
    pub 丁目コード: String,
    pub 小字コード: String,
    pub 予備コード: String,
    pub 大字名: Option<String>,
    pub 丁目名: Option<String>,
    pub 小字名: Option<String>,
    pub 予備名: Option<String>,
    pub 地番: String,
    pub 座標値種別: Option<String>,
    pub 筆界未定構成筆: Vec<筆界未定構成筆>,
    pub 代表点緯度: f64,
    pub 代表点経度: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "geoparquet", derive(GeoParquetRowStruct))]
pub struct 筆界未定構成筆 {
    pub 大字コード: String,
    pub 丁目コード: String,
    pub 小字コード: String,
    pub 予備コード: String,
    pub 大字名: Option<String>,
    pub 丁目名: Option<String>,
    pub 小字名: Option<String>,
    pub 予備名: Option<String>,
    pub 地番: String,
}

#[derive(Serialize)]
pub struct CommonProperties {
    pub 地図名: String,
    pub 市区町村コード: String,
    pub 市区町村名: String,
    pub 座標系: String,
    pub 測地系判別: Option<String>,
}

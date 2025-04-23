use crate::error::{Error, Result};

pub fn get_epsg_code(crs_name: &str) -> Result<Option<&'static str>> {
    match crs_name {
        "任意座標系" => Ok(None),
        "公共座標1系" => Ok(Some("EPSG:2443")), // JGD2000 / Japan Plane Rectangular CS I
        "公共座標2系" => Ok(Some("EPSG:2444")), // JGD2000 / Japan Plane Rectangular CS II
        "公共座標3系" => Ok(Some("EPSG:2445")), // JGD2000 / Japan Plane Rectangular CS III
        "公共座標4系" => Ok(Some("EPSG:2446")), // JGD2000 / Japan Plane Rectangular CS IV
        "公共座標5系" => Ok(Some("EPSG:2447")), // JGD2000 / Japan Plane Rectangular CS V
        "公共座標6系" => Ok(Some("EPSG:2448")), // JGD2000 / Japan Plane Rectangular CS VI
        "公共座標7系" => Ok(Some("EPSG:2449")), // JGD2000 / Japan Plane Rectangular CS VII
        "公共座標8系" => Ok(Some("EPSG:2450")), // JGD2000 / Japan Plane Rectangular CS VIII
        "公共座標9系" => Ok(Some("EPSG:2451")), // JGD2000 / Japan Plane Rectangular CS IX
        "公共座標10系" => Ok(Some("EPSG:2452")), // JGD2000 / Japan Plane Rectangular CS X
        "公共座標11系" => Ok(Some("EPSG:2453")), // JGD2000 / Japan Plane Rectangular CS XI
        "公共座標12系" => Ok(Some("EPSG:2454")), // JGD2000 / Japan Plane Rectangular CS XII
        "公共座標13系" => Ok(Some("EPSG:2455")), // JGD2000 / Japan Plane Rectangular CS XIII
        "公共座標14系" => Ok(Some("EPSG:2456")), // JGD2000 / Japan Plane Rectangular CS XIV
        "公共座標15系" => Ok(Some("EPSG:2457")), // JGD2000 / Japan Plane Rectangular CS XV
        "公共座標16系" => Ok(Some("EPSG:2458")), // JGD2000 / Japan Plane Rectangular CS XVI
        "公共座標17系" => Ok(Some("EPSG:2459")), // JGD2000 / Japan Plane Rectangular CS XVII
        "公共座標18系" => Ok(Some("EPSG:2460")), // JGD2000 / Japan Plane Rectangular CS XVIII
        "公共座標19系" => Ok(Some("EPSG:2461")), // JGD2000 / Japan Plane Rectangular CS XIX
        _ => Err(Error::UnsupportedCrs(crs_name.to_string())), // Error for unsupported CRS
    }
}

pub fn get_xml_namespace(prefix: Option<&str>) -> Option<&'static str> {
    match prefix {
        None => Some("http://www.moj.go.jp/MINJI/tizuxml"),
        Some("zmn") => Some("http://www.moj.go.jp/MINJI/tizuzumen"),
        Some("xsi") => Some("http://www.w3.org/2001/XMLSchema-instance"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_epsg_code_valid() -> Result<()> {
        assert_eq!(get_epsg_code("公共座標1系")?, Some("EPSG:2443"));
        assert_eq!(get_epsg_code("公共座標10系")?, Some("EPSG:2452"));
        assert_eq!(get_epsg_code("公共座標19系")?, Some("EPSG:2461"));
        Ok(())
    }

    #[test]
    fn test_get_epsg_code_arbitrary() -> Result<()> {
        assert_eq!(get_epsg_code("任意座標系")?, None);
        Ok(())
    }

    #[test]
    fn test_get_epsg_code_unknown() {
        assert_eq!(
            get_epsg_code("不明な座標系"),
            Err(Error::UnsupportedCrs("不明な座標系".to_string()))
        );
        assert_eq!(
            get_epsg_code(""),
            Err(Error::UnsupportedCrs("".to_string()))
        );
    }
}

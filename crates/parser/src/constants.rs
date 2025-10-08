use crate::error::{Error, Result};
use proj4rs::proj::Proj;
use std::collections::HashMap;
use std::sync::OnceLock;

static PROJ_STRS: &[(&str, &str); 20] = &[
    ("WGS84", "+proj=longlat +ellps=WGS84 +datum=WGS84 +no_defs"),
    (
        "公共座標1系", // 2443
        "+proj=tmerc +lat_0=33 +lon_0=129.5 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標2系", // 2444
        "+proj=tmerc +lat_0=33 +lon_0=131 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標3系", // 2445
        "+proj=tmerc +lat_0=36 +lon_0=132.166666666667 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標4系", // 2446
        "+proj=tmerc +lat_0=33 +lon_0=133.5 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標5系", // 2447
        "+proj=tmerc +lat_0=36 +lon_0=134.333333333333 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標6系", // 2448
        "+proj=tmerc +lat_0=36 +lon_0=136 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標7系", // 2449
        "+proj=tmerc +lat_0=36 +lon_0=137.166666666667 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標8系", // 2450
        "+proj=tmerc +lat_0=36 +lon_0=138.5 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標9系", // 2451
        "+proj=tmerc +lat_0=36 +lon_0=139.833333333333 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標10系", // 2452
        "+proj=tmerc +lat_0=40 +lon_0=140.833333333333 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標11系", // 2453
        "+proj=tmerc +lat_0=44 +lon_0=140.25 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標12系", // 2454
        "+proj=tmerc +lat_0=44 +lon_0=142.25 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標13系", // 2455
        "+proj=tmerc +lat_0=44 +lon_0=144.25 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標14系", // 2456
        "+proj=tmerc +lat_0=26 +lon_0=142 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標15系", // 2457
        "+proj=tmerc +lat_0=26 +lon_0=127.5 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標16系", // 2458
        "+proj=tmerc +lat_0=26 +lon_0=124 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標17系", // 2459
        "+proj=tmerc +lat_0=26 +lon_0=131 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標18系", // 2460
        "+proj=tmerc +lat_0=20 +lon_0=136 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
    (
        "公共座標19系", // 2461
        "+proj=tmerc +lat_0=26 +lon_0=154 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs +type=crs",
    ),
];

static PROJ_CACHE: OnceLock<HashMap<&'static str, OnceLock<Proj>>> = OnceLock::new();

pub fn get_proj(name: &str) -> Result<Option<&'static Proj>> {
    if name == "任意座標系" {
        return Ok(None);
    }
    let definition = PROJ_STRS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
        .ok_or_else(|| Error::UnsupportedCrs(name.to_string()))?;

    let cache = PROJ_CACHE.get_or_init(|| {
        let mut map = HashMap::with_capacity(PROJ_STRS.len());
        for (name, _) in PROJ_STRS.iter() {
            map.insert(*name, OnceLock::new());
        }
        map
    });

    let cell = cache
        .get(name)
        .ok_or_else(|| Error::UnsupportedCrs(name.to_string()))?;
    let proj = cell.get_or_init(|| {
        Proj::from_proj_string(definition)
            .unwrap_or_else(|err| panic!("invalid PROJ definition for {name}: {err:?}"))
    });
    Ok(Some(proj))
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
    use super::get_proj;
    use std::ptr;

    #[test]
    fn proj_cache_test() {
        let wgs84 = get_proj("WGS84")
            .expect("WGS84 lookup should succeed")
            .expect("WGS84 should not map to None");
        let wgs84_cached = get_proj("WGS84")
            .expect("WGS84 cached lookup should succeed")
            .expect("WGS84 cached lookup should not map to None");
        assert!(
            ptr::eq(wgs84, wgs84_cached),
            "WGS84 should return the same cached Proj instance"
        );

        let public_zone_1 = get_proj("公共座標1系")
            .expect("公共座標1系 lookup should succeed")
            .expect("公共座標1系 should not map to None");
        let public_zone_1_cached = get_proj("公共座標1系")
            .expect("公共座標1系 cached lookup should succeed")
            .expect("公共座標1系 cached lookup should not map to None");
        assert!(
            ptr::eq(public_zone_1, public_zone_1_cached),
            "公共座標1系 should return the same cached Proj instance"
        );

        assert!(
            !ptr::eq(wgs84, public_zone_1),
            "Distinct CRS names should resolve to distinct Proj instances"
        );
    }
}

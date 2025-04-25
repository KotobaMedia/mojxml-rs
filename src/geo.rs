use geo_types::{MultiPolygon, Point};

pub fn point_on_surface(mp: &MultiPolygon<f64>) -> Point<f64> {
    use geo::{
        Triangle,
        algorithm::{Area, Centroid, TriangulateEarcut},
    };

    // get the biggest polygon
    let polygon = mp
        .into_iter()
        .max_by(|a, b| a.unsigned_area().partial_cmp(&b.unsigned_area()).unwrap())
        .expect("MultiPolygon must have at least one Polygon");

    // (1) Triangulate into a Vec<Triangle<f64>>
    let triangles: Vec<Triangle<f64>> = polygon.earcut_triangles();

    // (2) Pick the triangle with the max area
    let largest = triangles
        .into_iter()
        .max_by(|a, b| a.unsigned_area().partial_cmp(&b.unsigned_area()).unwrap())
        .expect("polygon must have at least one triangle");

    // (3) Its centroid is interior
    

    largest.centroid()
}

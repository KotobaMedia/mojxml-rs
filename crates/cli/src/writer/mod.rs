mod flatgeobuf;
mod geojson;
mod geoparquet;
mod shared;

use anyhow::{Result, bail};
use std::path::Path;

use shared::Writer;

// Helper to lift `new` into a boxed factory fn.
pub trait Boxable: Writer + Sized + 'static {
    fn boxed_new(path: &Path) -> Result<Box<dyn Writer>> {
        Ok(Box::new(Self::new(path)?))
    }
}
// Blanket implementation
impl<T: Writer + Sized + 'static> Boxable for T {}

type WriterFactory = fn(&Path) -> Result<Box<dyn Writer>>;

// ---- Registry ----
static WRITER_MAP: &[(&str, WriterFactory)] = &[
    ("parquet", geoparquet::GeoParquetWriter::boxed_new),
    ("geojson", geojson::GeoJsonWriter::boxed_new),
    ("fgb", flatgeobuf::FlatGeobufWriter::boxed_new),
];

/// Make a Writer with a specified ID.
pub fn make_writer(id: &str, path: &Path) -> Result<Box<dyn Writer>> {
    if let Some((_, ctor)) = WRITER_MAP.iter().find(|(k, _)| *k == id) {
        return ctor(path);
    }
    bail!("unknown writer id: {id}")
}

/// Make a Writer based on the file extension of the output path.
pub fn make_writer_by_ext(path: &Path) -> Result<Box<dyn Writer>> {
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        return make_writer(ext, path);
    }
    bail!("cannot determine writer from path: {}", path.display())
}

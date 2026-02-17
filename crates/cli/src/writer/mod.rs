mod flatgeobuf;
mod geojson;
mod geoparquet;
mod shared;

use anyhow::{Result, bail};
use std::path::Path;

use shared::Writer;

#[derive(Debug, Clone, Copy)]
pub struct WriterOptions {
    pub fgb_write_index: bool,
}

impl Default for WriterOptions {
    fn default() -> Self {
        Self {
            fgb_write_index: true,
        }
    }
}

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

/// Make a Writer based on the file extension of the output path and writer options.
pub fn make_writer_by_ext_with_options(
    path: &Path,
    options: WriterOptions,
) -> Result<Box<dyn Writer>> {
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        if ext == "fgb" {
            return Ok(Box::new(
                flatgeobuf::FlatGeobufWriter::new_with_write_index(path, options.fgb_write_index)?,
            ));
        }
        return make_writer(ext, path);
    }
    bail!("cannot determine writer from path: {}", path.display())
}

// use proj::ProjCreateError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("XML parsing error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("Encoding error: {0}")]
    Encoding(#[from] std::str::Utf8Error),
    #[error("Missing required element: {0}")]
    MissingElement(String),
    #[error("Unsupported CRS: {0}")]
    UnsupportedCrs(String),
    #[error("Missing attribute '{attribute}' on element '{element}'")]
    MissingAttribute { element: String, attribute: String },
    #[error("Failed to parse float: {0}")]
    ParseFloat(#[from] std::num::ParseFloatError),
    #[error("Failed to find point referenced by ID: {0}")]
    PointNotFound(String),
    #[error("Unexpected XML element: {0}")]
    UnexpectedElement(String),
    #[error("Projection error: {0}")]
    Projection(#[from] proj4rs::errors::Error),
    #[error("IO error: {0}")]
    FS(#[from] std::io::Error),
    #[error("Parsing integer failed: {0}")]
    ParseInt(#[from] std::num::ParseIntError),
}

pub type Result<T> = std::result::Result<T, Error>;

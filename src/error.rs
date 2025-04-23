// use proj::ProjCreateError;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("XML parsing error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("Encoding error: {0}")]
    Encoding(#[from] std::str::Utf8Error),
    #[error("Coordinate parsing error: {0}")]
    Coordinate(String),
    #[error("Missing required element: {0}")]
    MissingElement(String),
    #[error("Invalid attribute value: {0}")]
    InvalidAttribute(String),
    #[error("Unsupported CRS: {0}")]
    UnsupportedCrs(String),
    #[error("Missing Namespace URI for prefix: {0}")]
    MissingNamespace(String),
    #[error("Node is not an element")]
    NotAnElement,
    #[error("Missing attribute '{attribute}' on element '{element}'")]
    MissingAttribute { element: String, attribute: String },
    #[error("Failed to parse float: {0}")]
    ParseFloat(#[from] std::num::ParseFloatError),
    #[error("Failed to find point referenced by ID: {0}")]
    PointNotFound(String),
    #[error("Unexpected XML element: {0}")]
    UnexpectedElement(String),
    #[error("Projection error: {0}")]
    Projection(String),
}

pub type Result<T> = std::result::Result<T, Error>;

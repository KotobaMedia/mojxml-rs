use std::path::Path;

use anyhow::Result;

use crate::parse::ParsedXML;

pub trait Writer: Send {
    /// Create a new instance
    fn new(output_path: &Path) -> Result<Self>
    where
        Self: Sized;

    /// Add features from the given ParsedXML to the output file.
    fn add_xml_features(&mut self, parsed: ParsedXML) -> Result<()>;

    /// Flush the writer and finalize the output file.
    /// This method must be called to ensure all data is written to the file.
    /// You cannot add any more features after calling this method.
    /// If no features were added, the file will be removed.
    /// The return value indicates whether the file was created (true) or not (false).
    fn flush(self: Box<Self>) -> Result<bool>;
}

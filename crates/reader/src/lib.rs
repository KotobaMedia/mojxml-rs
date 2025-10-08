use std::fs::File;
use std::io::{self, Cursor, Read, Seek};
use std::path::Path;
use zip::ZipArchive;
use zip::read::ZipFile;

type FileData = (String, String); // (file_name, contents)

#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

pub fn iter_xml_contents(
    path: &Path,
) -> Box<dyn Iterator<Item = Result<FileData, ReaderError>> + '_> {
    let ext = path
        .extension()
        .and_then(|os_str| os_str.to_str())
        .map(|s| s.to_lowercase());

    match ext.as_deref() {
        Some("xml") => Box::new(std::iter::once(read_xml_file(path))),
        Some("zip") => match read_zip_archive(path) {
            Ok(iter) => Box::new(iter),
            Err(e) => Box::new(std::iter::once(Err(e))),
        },
        _ => Box::new(std::iter::empty()),
    }
}

fn read_xml_file(path: &Path) -> Result<FileData, ReaderError> {
    let contents = std::fs::read_to_string(path)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    Ok((name, contents))
}

// streaming ZIP/XML iterator with nested ZIP support
struct ZipXmlIter<R: Read + Seek> {
    archive: ZipArchive<R>,
    index: usize,
    nested: Option<Box<ZipXmlIter<Cursor<Vec<u8>>>>>,
}

enum EntryDecision {
    Emit(FileData),
    QueueNested(ZipXmlIter<Cursor<Vec<u8>>>),
    Skip,
}

impl<R: Read + Seek> ZipXmlIter<R> {
    fn new(archive: ZipArchive<R>) -> Self {
        ZipXmlIter {
            archive,
            index: 0,
            nested: None,
        }
    }

    fn next_index(&mut self) -> Option<usize> {
        if self.index >= self.archive.len() {
            return None;
        }
        let idx = self.index;
        self.index += 1;
        Some(idx)
    }

    fn drain_nested(&mut self) -> Option<Result<FileData, ReaderError>> {
        if let Some(nested_iter) = &mut self.nested {
            if let Some(item) = nested_iter.next() {
                return Some(item);
            }
            self.nested = None;
        }
        None
    }

    fn process_entry(&mut self, index: usize) -> Result<EntryDecision, ReaderError> {
        let mut entry = self.archive.by_index(index)?;
        let entry_path = match entry.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => return Ok(EntryDecision::Skip),
        };

        if entry.is_dir() {
            return Ok(EntryDecision::Skip);
        }

        match Self::extension_lowercase(&entry_path).as_deref() {
            Some("xml") => Self::read_xml_entry(&mut entry, &entry_path).map(EntryDecision::Emit),
            Some("zip") => Self::build_nested_iterator(&mut entry)
                .map(|opt_iter| opt_iter.map_or(EntryDecision::Skip, EntryDecision::QueueNested)),
            _ => Ok(EntryDecision::Skip),
        }
    }

    fn read_xml_entry(
        entry: &mut ZipFile<'_, R>,
        entry_path: &Path,
    ) -> Result<FileData, ReaderError> {
        let name = entry_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        let mut contents = String::new();
        entry.read_to_string(&mut contents)?;

        Ok((name, contents))
    }

    fn build_nested_iterator(
        entry: &mut ZipFile<'_, R>,
    ) -> Result<Option<ZipXmlIter<Cursor<Vec<u8>>>>, ReaderError> {
        let mut buffer = Vec::new();
        entry.read_to_end(&mut buffer)?;

        if buffer.is_empty() {
            return Ok(None);
        }

        let cursor = Cursor::new(buffer);
        let nested_archive = ZipArchive::new(cursor)?;
        Ok(Some(ZipXmlIter::new(nested_archive)))
    }

    fn extension_lowercase(path: &Path) -> Option<String> {
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
    }
}

impl<R: Read + Seek> Iterator for ZipXmlIter<R> {
    type Item = Result<FileData, ReaderError>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.drain_nested() {
                return Some(item);
            }

            let idx = self.next_index()?;

            match self.process_entry(idx) {
                Ok(EntryDecision::Emit(data)) => return Some(Ok(data)),
                Ok(EntryDecision::QueueNested(nested)) => {
                    self.nested = Some(Box::new(nested));
                    continue;
                }
                Ok(EntryDecision::Skip) => continue,
                Err(err) => return Some(Err(err)),
            }
        }
    }
}

// replace read_zip_archive with streaming version
fn read_zip_archive(path: &Path) -> Result<ZipXmlIter<File>, ReaderError> {
    let file = File::open(path)?;
    let archive = ZipArchive::new(file)?;
    Ok(ZipXmlIter::new(archive))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_read_xml_file_success() {
        let mut path = testdata_path();
        path.push("46505-3411-56.xml");
        let result = read_xml_file(&path);
        assert!(result.is_ok());
        let file_data = result.unwrap();
        assert!(!file_data.1.is_empty());
        assert!(file_data.1.contains("<"));
    }

    #[test]
    fn test_read_xml_file_not_found() {
        let mut path = testdata_path();
        path.push("non_existent_file.xml");
        let result = read_xml_file(&path);
        assert!(result.is_err());
        match result.err().unwrap() {
            ReaderError::Io(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
            _ => panic!("Expected Io error"),
        }
    }

    #[test]
    fn test_read_zip_archive_success() {
        let mut path = testdata_path();
        path.push("46505-3411-1.zip");
        let result = read_zip_archive(&path);
        assert!(result.is_ok());
        let mut iter = result.unwrap();
        let first_item = iter.next();
        assert!(first_item.is_some());
        let first_data = first_item.unwrap();
        assert!(first_data.is_ok());
        let first_data = first_data.unwrap();
        assert_eq!(first_data.0, "46505-3411-1.xml");
        assert!(!first_data.1.is_empty());
    }

    #[test]
    fn test_read_zip_archive_multiple_xml() {
        let mut path = testdata_path();
        path.push("46505-3411-2025.zip");
        let result = read_zip_archive(&path);
        assert!(result.is_ok());
        let iter = result.unwrap();
        let items = iter.filter_map(|r| r.ok()).collect::<Vec<_>>();
        assert!(
            !items.is_empty(),
            "Expected at least one XML file in the zip"
        );
        let names = items.iter().map(|data| data.0.clone()).collect::<Vec<_>>();
        assert_eq!(names[0], "46505-3411-1.xml");
    }

    #[test]
    fn test_read_zip_archive_not_found() {
        let mut path = testdata_path();
        path.push("non_existent_archive.zip");
        let result = read_zip_archive(&path);
        assert!(result.is_err());
        match result.err().unwrap() {
            ReaderError::Io(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
            _ => panic!("Expected Io error"),
        }
    }

    #[test]
    fn test_read_zip_archive_invalid_zip() {
        let mut path = testdata_path();
        path.push("46505-3411-56.xml");
        let result = read_zip_archive(&path);
        assert!(result.is_err());
        match result.err().unwrap() {
            ReaderError::Zip(_) => {}
            _ => panic!("Expected Zip error"),
        }
    }

    #[test]
    fn test_iter_xml_contents_mixed_types() {
        let base_path = testdata_path();
        let paths = [
            base_path.join("46505-3411-56.xml"),
            base_path.join("46505-3411-1.zip"),
            base_path.join("non_existent_file.foo"),
            base_path.join("non_existent_file.xml"),
        ];

        let results: Vec<_> = paths.iter().flat_map(|p| iter_xml_contents(p)).collect();

        assert!(results.len() >= 2);
        assert!(results[0].is_ok());
        let buf = results[0].as_ref().unwrap().1.to_string();
        assert!(!buf.is_empty());
        let zip_results_ok = results.iter().skip(1).any(|r| r.is_ok());
        assert!(
            zip_results_ok,
            "Expected at least one successful read from the zip file"
        );
        let has_error = results.iter().any(|r| r.is_err());
        assert!(
            has_error,
            "Expected an error from the non-existent XML file"
        );
        let io_error_present = results.iter().any(|r| {
            if let Err(ReaderError::Io(e)) = r {
                e.kind() == io::ErrorKind::NotFound
            } else {
                false
            }
        });
        assert!(io_error_present, "Expected a NotFound IO error");
    }

    #[test]
    fn test_iter_xml_contents_only_zip() {
        let base_path = testdata_path();
        let paths = [base_path.join("46505-3411-1.zip")];
        let results: Vec<_> = paths.iter().flat_map(|p| iter_xml_contents(p)).collect();
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn test_iter_xml_contents_only_xml() {
        let base_path = testdata_path();
        let paths = [base_path.join("46505-3411-56.xml")];
        let results: Vec<_> = paths.iter().flat_map(|p| iter_xml_contents(p)).collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn test_iter_xml_contents_empty_input() {
        let paths: Vec<PathBuf> = vec![];
        let results: Vec<_> = paths.iter().flat_map(|p| iter_xml_contents(p)).collect();
        assert!(results.is_empty());
    }

    #[test]
    fn test_iter_xml_contents_ignore_other_files() {
        let base_path = testdata_path();
        let paths = [
            base_path.join("..").join("README.md"),
            base_path.join("..").join("Cargo.toml"),
        ];
        if paths.iter().all(|p| p.exists()) {
            let results: Vec<_> = paths.iter().flat_map(|p| iter_xml_contents(p)).collect();
            assert!(results.is_empty(), "Should ignore non-XML/ZIP files");
        } else {
            println!(
                "Skipping test_iter_xml_contents_ignore_other_files: Required non-XML/ZIP files not found."
            );
        }
    }
}

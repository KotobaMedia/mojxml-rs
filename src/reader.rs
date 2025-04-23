use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use tempfile::NamedTempFile;
use zip::ZipArchive;

pub struct FileData {
    pub file_name: String,
    pub contents: NamedTempFile,
}

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
    let mut tmp = NamedTempFile::new()?;
    let mut src = File::open(path)?;
    io::copy(&mut src, tmp.as_file_mut())?;
    tmp.as_file_mut().seek(SeekFrom::Start(0))?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(FileData {
        file_name: name,
        contents: tmp,
    })
}

// streaming ZIP/XML iterator without collecting
struct ZipXmlIter<R: Read + Seek> {
    archive: ZipArchive<R>,
    index: usize,
    nested: Option<Box<ZipXmlIter<std::fs::File>>>,
}

impl<R: Read + Seek> ZipXmlIter<R> {
    fn new(archive: ZipArchive<R>) -> Self {
        ZipXmlIter {
            archive,
            index: 0,
            nested: None,
        }
    }
}

impl<R: Read + Seek> Iterator for ZipXmlIter<R> {
    type Item = Result<FileData, ReaderError>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // first, drain any nested iterator
            if let Some(n) = &mut self.nested {
                if let Some(item) = n.next() {
                    return Some(item);
                }
                self.nested = None;
            }
            // if we've exhausted entries, stop
            if self.index >= self.archive.len() {
                return None;
            }
            let idx = self.index;
            self.index += 1;
            // pull the next entry
            let mut entry = match self.archive.by_index(idx) {
                Ok(e) => e,
                Err(e) => return Some(Err(ReaderError::Zip(e))),
            };
            let entry_path = match entry.enclosed_name() {
                Some(p) => p.to_path_buf(),
                None => continue,
            };
            let ext = entry_path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            match ext.as_deref() {
                Some("xml") => {
                    // emit XML immediately
                    match NamedTempFile::new() {
                        Ok(mut tmp) => {
                            if let Err(e) = io::copy(&mut entry, tmp.as_file_mut()) {
                                return Some(Err(ReaderError::Io(e)));
                            }
                            if let Err(e) = tmp.as_file_mut().seek(SeekFrom::Start(0)) {
                                return Some(Err(ReaderError::Io(e)));
                            }
                            let name = entry_path
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or_default()
                                .to_string();
                            return Some(Ok(FileData {
                                file_name: name,
                                contents: tmp,
                            }));
                        }
                        Err(e) => return Some(Err(ReaderError::Io(e))),
                    }
                }
                Some("zip") if !entry.is_dir() => {
                    // prepare nested ZIP iterator
                    match NamedTempFile::new() {
                        Ok(mut tmp) => {
                            if let Err(e) = io::copy(&mut entry, tmp.as_file_mut()) {
                                return Some(Err(ReaderError::Io(e)));
                            }
                            if let Err(e) = tmp.as_file_mut().seek(SeekFrom::Start(0)) {
                                return Some(Err(ReaderError::Io(e)));
                            }
                            // clone handle for ZipArchive
                            match tmp
                                .as_file()
                                .try_clone()
                                .and_then(|mut f| f.seek(SeekFrom::Start(0)).map(|_| f))
                            {
                                Ok(cloned_file) => match ZipArchive::new(cloned_file) {
                                    Ok(nested_arc) => {
                                        let mut nested_it = ZipXmlIter::new(nested_arc);
                                        if let Some(item) = nested_it.next() {
                                            self.nested = Some(Box::new(nested_it));
                                            return Some(item);
                                        } else {
                                            continue;
                                        }
                                    }
                                    Err(e) => return Some(Err(ReaderError::Zip(e))),
                                },
                                Err(e) => return Some(Err(ReaderError::Io(e))),
                            }
                        }
                        Err(e) => return Some(Err(ReaderError::Io(e))),
                    }
                }
                _ => continue,
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
    use std::io::Read;
    use std::path::PathBuf;

    fn testdata_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("testdata");
        path
    }

    #[test]
    fn test_read_xml_file_success() {
        let mut path = testdata_path();
        path.push("46505-3411-56.xml");
        let result = read_xml_file(&path);
        assert!(result.is_ok());
        let file_data = result.unwrap();
        let mut buf = Vec::new();
        file_data.contents.as_file().read_to_end(&mut buf).unwrap();
        assert!(!buf.is_empty());
        assert!(String::from_utf8_lossy(&buf).contains("<"));
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
        let mut buf = Vec::new();
        first_data
            .unwrap()
            .contents
            .as_file()
            .read_to_end(&mut buf)
            .unwrap();
        assert!(!buf.is_empty());
    }

    #[test]
    fn test_read_zip_archive_multiple_xml() {
        let mut path = testdata_path();
        path.push("46505-3411-2025.zip");
        let result = read_zip_archive(&path);
        assert!(result.is_ok());
        let iter = result.unwrap();
        let items = iter.filter_map(|r| r.ok()).collect::<Vec<_>>();
        assert!(items.len() > 0, "Expected at least one XML file in the zip");
        let names = items
            .iter()
            .map(|data| data.file_name.clone())
            .collect::<Vec<_>>();
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
        let paths = vec![
            base_path.join("46505-3411-56.xml"),
            base_path.join("46505-3411-1.zip"),
            base_path.join("non_existent_file.foo"),
            base_path.join("non_existent_file.xml"),
        ];

        let results: Vec<_> = paths.iter().flat_map(|p| iter_xml_contents(p)).collect();

        assert!(results.len() >= 2);
        assert!(results[0].is_ok());
        let mut buf = Vec::new();
        results[0]
            .as_ref()
            .unwrap()
            .contents
            .as_file()
            .read_to_end(&mut buf)
            .unwrap();
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
        let paths = vec![base_path.join("46505-3411-1.zip")];
        let results: Vec<_> = paths.iter().flat_map(|p| iter_xml_contents(p)).collect();
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn test_iter_xml_contents_only_xml() {
        let base_path = testdata_path();
        let paths = vec![base_path.join("46505-3411-56.xml")];
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
        let paths = vec![
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

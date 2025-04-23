use std::fs::File;
use std::io::{self, Cursor, Read, Seek};
use std::path::Path;
use zip::ZipArchive;

pub struct FileData {
    pub file_name: String,
    pub contents: Vec<u8>,
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
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(FileData {
        file_name: name,
        contents: buffer,
    })
}

fn find_xml_in_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    results: &mut Vec<Result<FileData, ReaderError>>,
) -> Result<(), ReaderError> {
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_path = match entry.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => continue,
        };

        let ext = entry_path
            .extension()
            .and_then(|os_str| os_str.to_str())
            .map(|s| s.to_lowercase());

        match ext.as_deref() {
            Some("xml") => {
                let mut buffer = Vec::new();
                match entry.read_to_end(&mut buffer) {
                    Ok(_) => {
                        let name = entry_path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_string();
                        results.push(Ok(FileData {
                            file_name: name,
                            contents: buffer,
                        }));
                    }
                    Err(e) => results.push(Err(ReaderError::Io(e))),
                }
            }
            Some("zip") => {
                let mut nested_zip_data = Vec::new();
                if entry.is_dir() {
                    continue;
                }
                match entry.read_to_end(&mut nested_zip_data) {
                    Ok(_) => {
                        if nested_zip_data.is_empty() {
                            continue;
                        }
                        let cursor = Cursor::new(nested_zip_data);
                        match ZipArchive::new(cursor) {
                            Ok(mut nested_archive) => {
                                find_xml_in_archive(&mut nested_archive, results)?;
                            }
                            Err(e) => results.push(Err(ReaderError::Zip(e))),
                        }
                    }
                    Err(e) => results.push(Err(ReaderError::Io(e))),
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn read_zip_archive(
    path: &Path,
) -> Result<impl Iterator<Item = Result<FileData, ReaderError>>, ReaderError> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut results = Vec::new();
    find_xml_in_archive(&mut archive, &mut results)?;
    Ok(results.into_iter())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(!file_data.contents.is_empty());
        assert!(String::from_utf8_lossy(&file_data.contents).contains("<"));
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
        assert!(!first_data.unwrap().contents.is_empty());
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
        assert!(!results[0].as_ref().unwrap().contents.is_empty());
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

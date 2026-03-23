use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use ignore::WalkBuilder;

use crate::contracts::{ByteRange, ContractError, ListPage, Provider, ReadPayload, ResolvedDocument};
use crate::domain::{DocumentId, DocumentLocator, DocumentMeta};

pub struct LocalFsProvider {
    root: PathBuf,
}

impl LocalFsProvider {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path_for_id(&self, document_id: &DocumentId) -> PathBuf {
        self.root.join(&document_id.0)
    }

    fn collect_files(&self, prefix: &str) -> Result<Vec<DocumentMeta>, ContractError> {
        let mut builder = WalkBuilder::new(&self.root);
        builder.standard_filters(false);
        builder.hidden(false);
        builder.git_ignore(false);
        builder.git_global(false);
        builder.git_exclude(false);

        let mut documents = Vec::new();
        for entry in builder.build() {
            let entry = entry.map_err(walk_error)?;
            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }

            let path = entry.path();
            let relative = relative_path(path, &self.root)?;
            if !prefix.is_empty() && !relative.starts_with(prefix) {
                continue;
            }
            documents.push(document_meta_from_path(&self.root, path)?);
        }
        Ok(documents)
    }
}

impl Provider for LocalFsProvider {
    fn list(&self, prefix: &str, cursor: Option<&str>) -> Result<ListPage, ContractError> {
        let mut documents = self.collect_files(prefix.trim_matches('/'))?;
        documents.sort_by(|left, right| left.locator.path.cmp(&right.locator.path));
        if let Some(cursor_value) = cursor {
            documents.retain(|document| document.locator.path.as_str() > cursor_value);
        }
        Ok(ListPage {
            documents,
            next_cursor: None,
        })
    }

    fn stat(&self, document_id: &DocumentId) -> Result<DocumentMeta, ContractError> {
        document_meta_from_path(&self.root, &self.path_for_id(document_id))
    }

    fn read(&self, document_id: &DocumentId, range: Option<ByteRange>) -> Result<ReadPayload, ContractError> {
        let path = self.path_for_id(document_id);
        let bytes = fs::read(path).map_err(io_error)?;
        let sliced = if let Some(range) = range {
            let start = usize::try_from(range.start).map_err(|_| ContractError::Invalid("range start overflow".to_owned()))?;
            let end = match range.end {
                Some(value) => usize::try_from(value).map_err(|_| ContractError::Invalid("range end overflow".to_owned()))?,
                None => bytes.len(),
            };
            if start > bytes.len() || end > bytes.len() || start > end {
                return Err(ContractError::Invalid("invalid byte range".to_owned()));
            }
            bytes[start..end].to_vec()
        } else {
            bytes
        };
        Ok(ReadPayload { bytes: sliced })
    }

    fn resolve(&self, locator: &DocumentLocator) -> Result<ResolvedDocument, ContractError> {
        let document_id = DocumentId(locator.path.clone());
        let meta = self.stat(&document_id)?;
        Ok(ResolvedDocument {
            id: document_id,
            locator: locator.clone(),
            meta: Some(meta),
        })
    }
}

fn document_meta_from_path(root: &Path, path: &Path) -> Result<DocumentMeta, ContractError> {
    let metadata = fs::metadata(path).map_err(io_error)?;
    let relative = relative_path(path, root)?;
    let extension = path.extension().map(|value| value.to_string_lossy().to_string());
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs().to_string());
    let version = modified_at
        .as_ref()
        .map(|value| format!("{value}-{}", metadata.len()));

    Ok(DocumentMeta {
        id: DocumentId(relative.clone()),
        locator: DocumentLocator { path: relative },
        extension: extension.clone(),
        content_type: guess_content_type(extension.as_deref()),
        version,
        size_bytes: metadata.len(),
        modified_at,
    })
}

fn guess_content_type(extension: Option<&str>) -> Option<String> {
    let content_type = match extension {
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("log") | Some("md") | Some("txt") => "text/plain",
        Some("mp4") => "video/mp4",
        _ => return None,
    };
    Some(content_type.to_owned())
}

fn io_error(error: std::io::Error) -> ContractError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ContractError::NotFound(error.to_string()),
        std::io::ErrorKind::PermissionDenied => ContractError::PermissionDenied(error.to_string()),
        _ => ContractError::Io(error.to_string()),
    }
}

fn walk_error(error: ignore::Error) -> ContractError {
    match error.io_error().map(std::io::Error::kind) {
        Some(std::io::ErrorKind::NotFound) => ContractError::NotFound(error.to_string()),
        Some(std::io::ErrorKind::PermissionDenied) => ContractError::PermissionDenied(error.to_string()),
        _ => ContractError::Io(error.to_string()),
    }
}

fn relative_path(path: &Path, root: &Path) -> Result<String, ContractError> {
    path.strip_prefix(root)
        .map_err(|_| ContractError::Invalid("path is outside provider root".to_owned()))
        .map(|value| value.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::contracts::{ByteRange, Provider};
    use crate::domain::{DocumentId, DocumentLocator};

    use super::LocalFsProvider;

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir().join("net-rga-local-fs-tests").join(format!("root-{nanos}"))
    }

    fn write_fixture(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| panic!("parent dirs should create: {error}"));
        }
        fs::write(path, content).unwrap_or_else(|error| panic!("fixture should write: {error}"));
    }

    #[test]
    fn list_filters_by_prefix() {
        let root = temp_root();
        write_fixture(&root, "docs/report.txt", "report");
        write_fixture(&root, "logs/app.log", "log");
        let provider = LocalFsProvider::new(root.clone());

        let page = provider
            .list("docs", None)
            .unwrap_or_else(|error| panic!("list should succeed: {error}"));

        assert_eq!(page.documents.len(), 1);
        assert_eq!(page.documents[0].locator.path, "docs/report.txt");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn stat_and_resolve_return_document_metadata() {
        let root = temp_root();
        write_fixture(&root, "docs/report.txt", "report");
        let provider = LocalFsProvider::new(root.clone());

        let stat = provider
            .stat(&DocumentId("docs/report.txt".to_owned()))
            .unwrap_or_else(|error| panic!("stat should succeed: {error}"));
        let resolved = provider
            .resolve(&DocumentLocator {
                path: "docs/report.txt".to_owned(),
            })
            .unwrap_or_else(|error| panic!("resolve should succeed: {error}"));

        assert_eq!(stat.locator.path, "docs/report.txt");
        assert_eq!(resolved.id.0, "docs/report.txt");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn read_supports_byte_ranges() {
        let root = temp_root();
        write_fixture(&root, "docs/report.txt", "abcdef");
        let provider = LocalFsProvider::new(root.clone());

        let payload = provider
            .read(
                &DocumentId("docs/report.txt".to_owned()),
                Some(ByteRange {
                    start: 1,
                    end: Some(4),
                }),
            )
            .unwrap_or_else(|error| panic!("read should succeed: {error}"));

        assert_eq!(payload.bytes, b"bcd");
        fs::remove_dir_all(root).ok();
    }
}

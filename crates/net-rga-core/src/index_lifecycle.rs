use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::config::StateLayout;
use crate::contracts::{ContractError, Provider};
use crate::extraction::ExtractorRegistry;
use crate::index::{IndexError, LexicalIndex};
use crate::runtime::RuntimePaths;
use crate::state::{ManifestDb, ManifestError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexStatus {
    pub present: bool,
    pub schema_version: Option<String>,
    pub update_strategy: Option<String>,
    pub backend_kind: Option<String>,
    pub last_build_started_at: Option<String>,
    pub last_build_completed_at: Option<String>,
    pub indexed_documents: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexBuildSummary {
    pub indexed_documents: u64,
    pub failed_documents: u64,
    pub last_build_started_at: String,
    pub last_build_completed_at: String,
}

#[derive(Debug, Error)]
pub enum IndexLifecycleError {
    #[error("{0}")]
    Runtime(#[from] crate::runtime::RuntimeError),
    #[error("{0}")]
    Manifest(#[from] ManifestError),
    #[error("{0}")]
    Index(#[from] IndexError),
    #[error("{0}")]
    Contract(#[from] ContractError),
    #[error("unknown corpus {0}")]
    UnknownCorpus(String),
}

pub fn build_index(
    paths: &RuntimePaths,
    corpus_id: &str,
    provider: &dyn Provider,
) -> Result<IndexBuildSummary, IndexLifecycleError> {
    let layout = index_layout(paths, corpus_id)?;
    let manifest = ManifestDb::open(&layout.manifest_db)?;
    let documents = manifest.list_documents(corpus_id)?;

    let index_path = layout.index_dir.join("index.db");
    let index = LexicalIndex::open(&index_path)?;
    let started_at = timestamp_now();
    index.write_health_metadata("last_build_started_at", &started_at)?;
    index.write_health_metadata("backend_kind", "embedded_sqlite_fts5")?;

    let mut indexed_documents = 0_u64;
    let mut failed_documents = 0_u64;

    for document in &documents {
        let payload = match provider.read(&document.id, None) {
            Ok(payload) => payload,
            Err(_) => {
                failed_documents += 1;
                continue;
            }
        };

        let canonical = match ExtractorRegistry::extract(document, &payload.bytes, &[]) {
            Ok(canonical) => canonical,
            Err(_) => {
                failed_documents += 1;
                continue;
            }
        };

        index.upsert_document(document, &canonical)?;
        indexed_documents += 1;
    }

    let completed_at = timestamp_now();
    index.write_health_metadata("last_build_completed_at", &completed_at)?;
    index.write_health_metadata("indexed_documents", &indexed_documents.to_string())?;

    Ok(IndexBuildSummary {
        indexed_documents,
        failed_documents,
        last_build_started_at: started_at,
        last_build_completed_at: completed_at,
    })
}

pub fn rebuild_index(
    paths: &RuntimePaths,
    corpus_id: &str,
    provider: &dyn Provider,
) -> Result<IndexBuildSummary, IndexLifecycleError> {
    let layout = index_layout(paths, corpus_id)?;
    clear_index_file(&layout.index_dir.join("index.db"))?;
    build_index(paths, corpus_id, provider)
}

pub fn clear_index(paths: &RuntimePaths, corpus_id: &str) -> Result<bool, IndexLifecycleError> {
    let layout = index_layout(paths, corpus_id)?;
    let index_path = layout.index_dir.join("index.db");
    if !index_path.exists() {
        return Ok(false);
    }
    clear_index_file(&index_path)?;
    Ok(true)
}

pub fn index_status(
    paths: &RuntimePaths,
    corpus_id: &str,
) -> Result<IndexStatus, IndexLifecycleError> {
    let layout = index_layout(paths, corpus_id)?;
    let index_path = layout.index_dir.join("index.db");
    if !index_path.exists() {
        return Ok(IndexStatus {
            present: false,
            schema_version: None,
            update_strategy: None,
            backend_kind: None,
            last_build_started_at: None,
            last_build_completed_at: None,
            indexed_documents: 0,
        });
    }

    let index = LexicalIndex::open_read_only(&index_path)?;
    let indexed_documents = index.indexed_document_count()?;
    Ok(IndexStatus {
        present: true,
        schema_version: index.schema_version()?,
        update_strategy: index.update_strategy()?,
        backend_kind: index.read_health_metadata("backend_kind")?,
        last_build_started_at: index.read_health_metadata("last_build_started_at")?,
        last_build_completed_at: index.read_health_metadata("last_build_completed_at")?,
        indexed_documents,
    })
}

fn clear_index_file(path: &Path) -> Result<(), IndexLifecycleError> {
    match fs::remove_file(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(IndexLifecycleError::Index(IndexError::Io(error))),
    }
}

fn index_layout(paths: &RuntimePaths, corpus_id: &str) -> Result<StateLayout, IndexLifecycleError> {
    let store = crate::runtime::ConfigStore::new(paths.clone());
    let exists = store
        .list_corpora()?
        .into_iter()
        .any(|corpus| corpus.id == corpus_id);
    if !exists {
        return Err(IndexLifecycleError::UnknownCorpus(corpus_id.to_owned()));
    }
    Ok(StateLayout::for_corpus(
        &paths.state_root,
        &crate::domain::CorpusId(corpus_id.to_owned()),
    ))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{CorpusConfig, ProviderConfig};
    use crate::runtime::{ConfigStore, RuntimePaths};

    use super::{clear_index, index_status};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_state_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir()
            .join("net-rga-index-lifecycle-tests")
            .join(format!("state-{}-{nanos}-{counter}", std::process::id()))
    }

    #[test]
    fn index_status_reports_absent_index_for_known_corpus() {
        let state_root = temp_state_root();
        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
        store
            .add_corpus(CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: PathBuf::from("/tmp/docs"),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            })
            .unwrap_or_else(|error| panic!("corpus should save: {error}"));

        let status = index_status(&paths, "local")
            .unwrap_or_else(|error| panic!("status should succeed: {error}"));
        assert!(!status.present);
        assert_eq!(status.indexed_documents, 0);

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn clear_index_returns_false_when_index_is_missing() {
        let state_root = temp_state_root();
        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
        store
            .add_corpus(CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: PathBuf::from("/tmp/docs"),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            })
            .unwrap_or_else(|error| panic!("corpus should save: {error}"));

        let removed = clear_index(&paths, "local")
            .unwrap_or_else(|error| panic!("clear should succeed: {error}"));
        assert!(!removed);

        fs::remove_dir_all(state_root).ok();
    }
}

fn timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

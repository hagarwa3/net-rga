use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::config::{CorpusConfig, ProviderConfig, StateLayout};
use crate::contracts::{ContractError, Provider};
use crate::providers::{LocalFsProvider, S3ConnectionConfig, S3Provider};
use crate::runtime::{ConfigStore, RuntimeError, RuntimePaths};
use crate::state::{ManifestDb, ManifestError};

const CHECKPOINT_LIST_CURSOR: &str = "list_cursor";
const CHECKPOINT_LAST_SYNC_STARTED_AT: &str = "last_sync_started_at";
const CHECKPOINT_LAST_SYNC_COMPLETED_AT: &str = "last_sync_completed_at";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncCheckpointName {
    ListCursor,
    LastSyncStartedAt,
    LastSyncCompletedAt,
}

impl SyncCheckpointName {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ListCursor => CHECKPOINT_LIST_CURSOR,
            Self::LastSyncStartedAt => CHECKPOINT_LAST_SYNC_STARTED_AT,
            Self::LastSyncCompletedAt => CHECKPOINT_LAST_SYNC_COMPLETED_AT,
        }
    }
}

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("{0}")]
    Runtime(#[from] RuntimeError),
    #[error("{0}")]
    Manifest(#[from] ManifestError),
    #[error("{0}")]
    Contract(#[from] ContractError),
    #[error("toml serialization error: {0}")]
    SerializeToml(#[from] toml::ser::Error),
    #[error("corpus not found: {0}")]
    CorpusNotFound(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRunSummary {
    pub corpus_id: String,
    pub started_at: String,
    pub completed_at: String,
    pub listed_documents: u64,
    pub pages_processed: u64,
    pub last_cursor: Option<String>,
}

pub fn sync_corpus(paths: &RuntimePaths, corpus_id: &str) -> Result<SyncRunSummary, SyncError> {
    let store = ConfigStore::new(paths.clone());
    let corpus = store
        .list_corpora()?
        .into_iter()
        .find(|candidate| candidate.id == corpus_id)
        .ok_or_else(|| SyncError::CorpusNotFound(corpus_id.to_owned()))?;

    sync_corpus_config(paths, &corpus)
}

pub fn sync_corpus_config(paths: &RuntimePaths, corpus: &CorpusConfig) -> Result<SyncRunSummary, SyncError> {
    let provider = provider_for_config(&corpus.provider)?;
    let layout = StateLayout::for_corpus(&paths.state_root, &crate::domain::CorpusId(corpus.id.clone()));
    let manifest = ManifestDb::open(&layout.manifest_db)?;
    let started_at = timestamp_now();

    manifest.upsert_corpus(
        corpus,
        provider_kind_label(&corpus.provider),
        &provider_root(&corpus.provider),
        &toml::to_string(corpus)?,
        &started_at,
    )?;
    manifest.upsert_sync_checkpoint(
        &corpus.id,
        SyncCheckpointName::LastSyncStartedAt.as_str(),
        Some(&started_at),
        &started_at,
    )?;

    let mut cursor = None;
    let mut listed_documents = 0_u64;
    let mut pages_processed = 0_u64;

    loop {
        let page = provider.list("", cursor.as_deref())?;
        for document in &page.documents {
            manifest.upsert_document(&corpus.id, document, &started_at)?;
        }
        listed_documents += u64::try_from(page.documents.len()).unwrap_or_default();
        pages_processed += 1;
        cursor = page.next_cursor.clone();
        manifest.upsert_sync_checkpoint(
            &corpus.id,
            SyncCheckpointName::ListCursor.as_str(),
            cursor.as_deref(),
            &timestamp_now(),
        )?;
        if cursor.is_none() {
            break;
        }
    }

    let completed_at = timestamp_now();
    manifest.tombstone_missing_documents(&corpus.id, &started_at, &completed_at)?;
    manifest.upsert_sync_checkpoint(
        &corpus.id,
        SyncCheckpointName::LastSyncCompletedAt.as_str(),
        Some(&completed_at),
        &completed_at,
    )?;

    Ok(SyncRunSummary {
        corpus_id: corpus.id.clone(),
        started_at,
        completed_at,
        listed_documents,
        pages_processed,
        last_cursor: None,
    })
}

fn provider_for_config(config: &ProviderConfig) -> Result<Box<dyn Provider>, ContractError> {
    match config {
        ProviderConfig::LocalFs { root } => Ok(Box::new(LocalFsProvider::new(root.clone()))),
        ProviderConfig::S3 { .. } => {
            let connection = S3ConnectionConfig::from_provider_config(config)?;
            Ok(Box::new(S3Provider::new(connection)?))
        }
    }
}

fn provider_kind_label(config: &ProviderConfig) -> &'static str {
    match config {
        ProviderConfig::LocalFs { .. } => "local_fs",
        ProviderConfig::S3 { endpoint: Some(_), .. } => "s3_compatible",
        ProviderConfig::S3 { .. } => "s3",
    }
}

fn provider_root(config: &ProviderConfig) -> String {
    match config {
        ProviderConfig::LocalFs { root } => root.display().to_string(),
        ProviderConfig::S3 { bucket, prefix, .. } => match prefix {
            Some(value) if !value.is_empty() => format!("s3://{bucket}/{}", value.trim_matches('/')),
            _ => format!("s3://{bucket}"),
        },
    }
}

fn timestamp_now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{SyncCheckpointName, sync_corpus};
    use crate::config::{CorpusConfig, ProviderConfig};
    use crate::runtime::{ConfigStore, RuntimePaths};
    use crate::state::ManifestDb;

    fn temp_state_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir().join("net-rga-sync-tests").join(format!("state-{nanos}"))
    }

    #[test]
    fn sync_persists_checkpoints_for_local_corpus() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        fs::write(corpus_root.join("docs/report.txt"), "riverglass")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
        store
            .add_corpus(CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: corpus_root.clone(),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            })
            .unwrap_or_else(|error| panic!("corpus should save: {error}"));

        let summary = sync_corpus(&paths, "local")
            .unwrap_or_else(|error| panic!("sync should succeed: {error}"));
        assert_eq!(summary.corpus_id, "local");
        assert!(summary.pages_processed >= 1);

        let manifest = ManifestDb::open(&state_root.join("corpora/local/manifest.db"))
            .unwrap_or_else(|error| panic!("manifest should open: {error}"));
        let started = manifest
            .sync_checkpoint("local", SyncCheckpointName::LastSyncStartedAt.as_str())
            .unwrap_or_else(|error| panic!("start checkpoint should load: {error}"));
        let completed = manifest
            .sync_checkpoint("local", SyncCheckpointName::LastSyncCompletedAt.as_str())
            .unwrap_or_else(|error| panic!("completion checkpoint should load: {error}"));

        assert!(started.is_some());
        assert!(completed.is_some());
        assert_eq!(
            manifest
                .document_count("local")
                .unwrap_or_else(|error| panic!("document count should query: {error}")),
            1
        );

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn sync_tombstones_deleted_local_documents() {
        let state_root = temp_state_root();
        let corpus_root = state_root.join("fixtures");
        fs::create_dir_all(corpus_root.join("docs"))
            .unwrap_or_else(|error| panic!("fixture dir should create: {error}"));
        let report_path = corpus_root.join("docs/report.txt");
        fs::write(&report_path, "riverglass")
            .unwrap_or_else(|error| panic!("fixture should write: {error}"));

        let paths = RuntimePaths::from_state_root(state_root.clone());
        let store = ConfigStore::new(paths.clone());
        store
            .add_corpus(CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: corpus_root.clone(),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            })
            .unwrap_or_else(|error| panic!("corpus should save: {error}"));

        sync_corpus(&paths, "local").unwrap_or_else(|error| panic!("first sync should succeed: {error}"));
        fs::remove_file(report_path).unwrap_or_else(|error| panic!("fixture should delete: {error}"));
        sync_corpus(&paths, "local").unwrap_or_else(|error| panic!("second sync should succeed: {error}"));

        let manifest = ManifestDb::open(&state_root.join("corpora/local/manifest.db"))
            .unwrap_or_else(|error| panic!("manifest should open: {error}"));
        assert_eq!(
            manifest
                .document_count("local")
                .unwrap_or_else(|error| panic!("document count should query: {error}")),
            0
        );
        assert_eq!(
            manifest
                .tombstone_count("local")
                .unwrap_or_else(|error| panic!("tombstone count should query: {error}")),
            1
        );

        fs::remove_dir_all(state_root).ok();
    }
}

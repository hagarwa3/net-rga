use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{CorpusConfig, StateLayout};
use crate::domain::CorpusId;
use crate::runtime::{ConfigStore, RuntimePaths};

pub const BUNDLE_SCHEMA_VERSION: &str = "1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub schema_version: String,
    pub corpus: BundleCorpus,
    pub artifacts: BundleArtifacts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleCorpus {
    pub id: String,
    pub display_name: Option<String>,
    pub provider_kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleArtifacts {
    pub corpus_config: String,
    pub manifest_db: String,
    pub index_dir: Option<String>,
    pub cache_dir: Option<String>,
}

impl BundleManifest {
    pub fn for_corpus(config: &CorpusConfig, include_index: bool, include_cache: bool) -> Self {
        Self {
            schema_version: BUNDLE_SCHEMA_VERSION.to_owned(),
            corpus: BundleCorpus {
                id: config.id.clone(),
                display_name: config.display_name.clone(),
                provider_kind: match config.provider {
                    crate::config::ProviderConfig::LocalFs { .. } => "local_fs".to_owned(),
                    crate::config::ProviderConfig::S3 { .. } => "s3".to_owned(),
                },
            },
            artifacts: BundleArtifacts {
                corpus_config: "corpus.toml".to_owned(),
                manifest_db: "manifest.db".to_owned(),
                index_dir: include_index.then(|| "index".to_owned()),
                cache_dir: include_cache.then(|| "cache".to_owned()),
            },
        }
    }

    pub fn validate(&self) -> Result<(), BundleError> {
        if self.schema_version != BUNDLE_SCHEMA_VERSION {
            return Err(BundleError::UnsupportedSchema(self.schema_version.clone()));
        }
        if self.artifacts.corpus_config.is_empty() || self.artifacts.manifest_db.is_empty() {
            return Err(BundleError::InvalidManifest(
                "bundle manifest must define config and manifest paths".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundlePayload {
    pub manifest: BundleManifest,
    pub corpus_config: CorpusConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("unsupported bundle schema version: {0}")]
    UnsupportedSchema(String),
    #[error("invalid bundle manifest: {0}")]
    InvalidManifest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml serialization error: {0}")]
    SerializeToml(#[from] toml::ser::Error),
    #[error("toml deserialization error: {0}")]
    DeserializeToml(#[from] toml::de::Error),
    #[error("{0}")]
    Runtime(#[from] crate::runtime::RuntimeError),
}

pub fn write_bundle(
    bundle_root: &Path,
    payload: &BundlePayload,
    layout: &StateLayout,
) -> Result<(), BundleError> {
    payload.manifest.validate()?;
    fs::create_dir_all(bundle_root)?;

    let bundle_manifest_path = bundle_root.join("bundle.json");
    fs::write(
        bundle_manifest_path,
        serde_json::to_string_pretty(&payload.manifest)?,
    )?;

    let corpus_config_path = bundle_root.join(&payload.manifest.artifacts.corpus_config);
    fs::write(corpus_config_path, toml::to_string_pretty(&payload.corpus_config)?)?;

    let manifest_target = bundle_root.join(&payload.manifest.artifacts.manifest_db);
    copy_file(&layout.manifest_db, &manifest_target)?;

    if let Some(index_dir) = payload.manifest.artifacts.index_dir.as_deref() {
        copy_dir_recursive(&layout.index_dir, &bundle_root.join(index_dir))?;
    }
    if let Some(cache_dir) = payload.manifest.artifacts.cache_dir.as_deref() {
        copy_dir_recursive(&layout.cache_dir, &bundle_root.join(cache_dir))?;
    }
    Ok(())
}

pub fn read_bundle(bundle_root: &Path) -> Result<BundlePayload, BundleError> {
    let manifest_path = bundle_root.join("bundle.json");
    let manifest: BundleManifest = serde_json::from_str(&fs::read_to_string(manifest_path)?)?;
    manifest.validate()?;
    let corpus_config_path = bundle_root.join(&manifest.artifacts.corpus_config);
    let corpus_config: CorpusConfig = toml::from_str(&fs::read_to_string(corpus_config_path)?)?;
    Ok(BundlePayload {
        manifest,
        corpus_config,
    })
}

pub fn bundle_artifact_path(bundle_root: &Path, relative_path: &str) -> PathBuf {
    bundle_root.join(relative_path)
}

pub fn export_corpus_bundle(
    paths: &RuntimePaths,
    corpus_id: &str,
    bundle_root: &Path,
) -> Result<BundleManifest, BundleError> {
    let store = ConfigStore::new(paths.clone());
    let corpus = store
        .list_corpora()?
        .into_iter()
        .find(|candidate| candidate.id == corpus_id)
        .ok_or_else(|| BundleError::InvalidManifest(format!("unknown corpus {corpus_id}")))?;
    let layout = StateLayout::for_corpus(&paths.state_root, &CorpusId(corpus.id.clone()));
    let include_index = directory_has_entries(&layout.index_dir)?;
    let include_cache = directory_has_entries(&layout.cache_dir)?;
    let payload = BundlePayload {
        manifest: BundleManifest::for_corpus(&corpus, include_index, include_cache),
        corpus_config: corpus,
    };
    write_bundle(bundle_root, &payload, &layout)?;
    Ok(payload.manifest)
}

pub fn import_corpus_bundle(
    paths: &RuntimePaths,
    bundle_root: &Path,
) -> Result<BundleManifest, BundleError> {
    let payload = read_bundle(bundle_root)?;
    let corpus_id = CorpusId(payload.corpus_config.id.clone());
    let layout = StateLayout::for_corpus(&paths.state_root, &corpus_id);
    fs::create_dir_all(&layout.corpus_root)?;
    copy_file(
        &bundle_artifact_path(bundle_root, &payload.manifest.artifacts.manifest_db),
        &layout.manifest_db,
    )?;

    restore_optional_directory(
        bundle_root,
        payload.manifest.artifacts.index_dir.as_deref(),
        &layout.index_dir,
    )?;
    restore_optional_directory(
        bundle_root,
        payload.manifest.artifacts.cache_dir.as_deref(),
        &layout.cache_dir,
    )?;

    let store = ConfigStore::new(paths.clone());
    store.upsert_corpus(payload.corpus_config)?;
    Ok(payload.manifest)
}

fn copy_file(source: &Path, target: &Path) -> Result<(), BundleError> {
    let parent = target.parent().ok_or_else(|| {
        BundleError::InvalidManifest(format!("invalid target path: {}", target.display()))
    })?;
    fs::create_dir_all(parent)?;
    fs::copy(source, target)?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), BundleError> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else {
            copy_file(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn directory_has_entries(path: &Path) -> Result<bool, BundleError> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(fs::read_dir(path)?.next().is_some())
}

fn restore_optional_directory(
    bundle_root: &Path,
    bundle_relative: Option<&str>,
    target: &Path,
) -> Result<(), BundleError> {
    if target.exists() {
        fs::remove_dir_all(target)?;
    }

    let Some(bundle_relative) = bundle_relative else {
        return Ok(());
    };
    let source = bundle_artifact_path(bundle_root, bundle_relative);
    if !source.exists() {
        return Ok(());
    }
    copy_dir_recursive(&source, target)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{CorpusConfig, ProviderConfig};
    use crate::config::StateLayout;

    use crate::runtime::RuntimePaths;

    use super::{
        BUNDLE_SCHEMA_VERSION, BundleManifest, BundlePayload, import_corpus_bundle, read_bundle,
        write_bundle,
    };

    #[test]
    fn bundle_manifest_tracks_optional_artifacts() {
        let manifest = BundleManifest::for_corpus(
            &CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: PathBuf::from("/data"),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            },
            true,
            false,
        );

        assert_eq!(manifest.schema_version, BUNDLE_SCHEMA_VERSION);
        assert_eq!(manifest.artifacts.index_dir.as_deref(), Some("index"));
        assert_eq!(manifest.artifacts.cache_dir, None);
    }

    #[test]
    fn bundle_manifest_rejects_unknown_schema_versions() {
        let invalid = BundleManifest {
            schema_version: "99".to_owned(),
            corpus: super::BundleCorpus {
                id: "local".to_owned(),
                display_name: None,
                provider_kind: "local_fs".to_owned(),
            },
            artifacts: super::BundleArtifacts {
                corpus_config: "corpus.toml".to_owned(),
                manifest_db: "manifest.db".to_owned(),
                index_dir: None,
                cache_dir: None,
            },
        };

        assert!(invalid.validate().is_err());
    }

    #[test]
    fn bundle_manifest_requires_core_artifact_paths() {
        let invalid = BundleManifest {
            schema_version: BUNDLE_SCHEMA_VERSION.to_owned(),
            corpus: super::BundleCorpus {
                id: "local".to_owned(),
                display_name: None,
                provider_kind: "local_fs".to_owned(),
            },
            artifacts: super::BundleArtifacts {
                corpus_config: String::new(),
                manifest_db: String::new(),
                index_dir: None,
                cache_dir: None,
            },
        };

        assert!(invalid.validate().is_err());
    }

    #[test]
    fn bundle_write_and_read_round_trip_manifest_and_config() {
        let root = temp_root();
        let bundle_root = root.join("bundle");
        let layout = StateLayout {
            state_root: root.clone(),
            corpus_root: root.join("corpora/local"),
            manifest_db: root.join("corpora/local/manifest.db"),
            index_dir: root.join("corpora/local/index"),
            cache_dir: root.join("corpora/local/cache"),
        };
        fs::create_dir_all(&layout.index_dir)
            .unwrap_or_else(|error| panic!("index dir should create: {error}"));
        fs::write(&layout.manifest_db, "manifest-bytes")
            .unwrap_or_else(|error| panic!("manifest should write: {error}"));
        fs::write(layout.index_dir.join("index.db"), "index-bytes")
            .unwrap_or_else(|error| panic!("index should write: {error}"));

        let corpus = CorpusConfig {
            id: "local".to_owned(),
            display_name: Some("Local".to_owned()),
            provider: ProviderConfig::LocalFs {
                root: PathBuf::from("/data"),
            },
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            backend: None,
        };
        let payload = BundlePayload {
            manifest: BundleManifest::for_corpus(&corpus, true, false),
            corpus_config: corpus,
        };

        write_bundle(&bundle_root, &payload, &layout)
            .unwrap_or_else(|error| panic!("bundle should write: {error}"));
        let loaded = read_bundle(&bundle_root)
            .unwrap_or_else(|error| panic!("bundle should read: {error}"));

        assert_eq!(loaded.manifest, payload.manifest);
        assert_eq!(loaded.corpus_config.id, "local");
        assert!(bundle_root.join("manifest.db").exists());
        assert!(bundle_root.join("index/index.db").exists());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn import_tolerates_missing_optional_index_and_cache_directories() {
        let root = temp_root();
        let bundle_root = root.join("bundle");
        let import_root = root.join("imported");
        let layout = StateLayout {
            state_root: root.clone(),
            corpus_root: root.join("corpora/local"),
            manifest_db: root.join("corpora/local/manifest.db"),
            index_dir: root.join("corpora/local/index"),
            cache_dir: root.join("corpora/local/cache"),
        };
        fs::create_dir_all(&layout.index_dir)
            .unwrap_or_else(|error| panic!("index dir should create: {error}"));
        fs::create_dir_all(&layout.cache_dir)
            .unwrap_or_else(|error| panic!("cache dir should create: {error}"));
        fs::write(&layout.manifest_db, "manifest-bytes")
            .unwrap_or_else(|error| panic!("manifest should write: {error}"));
        fs::write(layout.index_dir.join("index.db"), "index-bytes")
            .unwrap_or_else(|error| panic!("index should write: {error}"));
        fs::write(layout.cache_dir.join("entry.txt"), "cache-bytes")
            .unwrap_or_else(|error| panic!("cache should write: {error}"));

        let corpus = CorpusConfig {
            id: "local".to_owned(),
            display_name: Some("Local".to_owned()),
            provider: ProviderConfig::LocalFs {
                root: PathBuf::from("/data"),
            },
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            backend: None,
        };
        let payload = BundlePayload {
            manifest: BundleManifest::for_corpus(&corpus, true, true),
            corpus_config: corpus,
        };
        write_bundle(&bundle_root, &payload, &layout)
            .unwrap_or_else(|error| panic!("bundle should write: {error}"));
        fs::remove_dir_all(bundle_root.join("index")).ok();
        fs::remove_dir_all(bundle_root.join("cache")).ok();

        let imported = import_corpus_bundle(&RuntimePaths::from_state_root(import_root.clone()), &bundle_root)
            .unwrap_or_else(|error| panic!("bundle import should succeed without optional dirs: {error}"));

        let imported_layout = StateLayout::for_corpus(&import_root, &crate::domain::CorpusId("local".to_owned()));
        assert_eq!(imported.corpus.id, "local");
        assert!(imported_layout.manifest_db.exists());
        assert!(!imported_layout.index_dir.exists());
        assert!(!imported_layout.cache_dir.exists());

        fs::remove_dir_all(root).ok();
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir()
            .join("net-rga-bundle-tests")
            .join(format!("bundle-{nanos}"))
    }
}

use serde::{Deserialize, Serialize};

use crate::config::CorpusConfig;

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
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::config::{CorpusConfig, ProviderConfig};

    use super::{BUNDLE_SCHEMA_VERSION, BundleManifest};

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
}

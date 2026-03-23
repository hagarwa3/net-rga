use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::CorpusId;

pub const DEFAULT_STATE_DIR_NAME: &str = ".net-rga";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub state_root: Option<PathBuf>,
    pub default_corpus: Option<String>,
    pub corpora: Vec<CorpusConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusConfig {
    pub id: String,
    pub display_name: Option<String>,
    pub provider: ProviderConfig,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    pub backend: Option<BackendBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderConfig {
    LocalFs {
        root: PathBuf,
    },
    S3 {
        bucket: String,
        prefix: Option<String>,
        region: Option<String>,
        endpoint: Option<String>,
        profile: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendBinding {
    pub kind: String,
    pub endpoint: String,
    pub index_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateLayout {
    pub state_root: PathBuf,
    pub corpus_root: PathBuf,
    pub manifest_db: PathBuf,
    pub index_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl StateLayout {
    pub fn for_corpus(state_root: &Path, corpus_id: &CorpusId) -> Self {
        let corpus_dir_name = sanitize_corpus_id(&corpus_id.0);
        let corpus_root = state_root.join("corpora").join(corpus_dir_name);
        Self {
            state_root: state_root.to_path_buf(),
            manifest_db: corpus_root.join("manifest.db"),
            index_dir: corpus_root.join("index"),
            cache_dir: corpus_root.join("cache"),
            corpus_root,
        }
    }

    pub fn default_state_root(home_dir: &Path) -> PathBuf {
        home_dir.join(DEFAULT_STATE_DIR_NAME)
    }
}

fn sanitize_corpus_id(value: &str) -> String {
    value.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{AppConfig, CorpusConfig, ProviderConfig, StateLayout};
    use crate::domain::CorpusId;

    #[test]
    fn config_round_trips_through_toml() {
        let config = AppConfig {
            state_root: Some(PathBuf::from("/tmp/net-rga")),
            default_corpus: Some("local".to_owned()),
            corpora: vec![CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: PathBuf::from("/data"),
                },
                include_globs: vec!["docs/**".to_owned()],
                exclude_globs: vec!["media/**".to_owned()],
                backend: None,
            }],
        };

        let encoded = match toml::to_string(&config) {
            Ok(value) => value,
            Err(error) => panic!("config should serialize: {error}"),
        };
        let decoded: AppConfig = match toml::from_str(&encoded) {
            Ok(value) => value,
            Err(error) => panic!("config should deserialize: {error}"),
        };

        assert_eq!(decoded.default_corpus.as_deref(), Some("local"));
        assert_eq!(decoded.corpora.len(), 1);
        assert_eq!(decoded.corpora[0].include_globs, vec!["docs/**"]);
    }

    #[test]
    fn state_layout_uses_corpus_specific_directories() {
        let layout = StateLayout::for_corpus(
            Path::new("/tmp/net-rga"),
            &CorpusId("tier0.local_fs".to_owned()),
        );

        assert_eq!(layout.manifest_db, PathBuf::from("/tmp/net-rga/corpora/tier0_local_fs/manifest.db"));
        assert_eq!(layout.index_dir, PathBuf::from("/tmp/net-rga/corpora/tier0_local_fs/index"));
        assert_eq!(layout.cache_dir, PathBuf::from("/tmp/net-rga/corpora/tier0_local_fs/cache"));
    }
}

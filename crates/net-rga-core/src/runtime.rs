use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::{AppConfig, CorpusConfig, StateLayout};

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("HOME is not set and NET_RGA_STATE_ROOT was not provided")]
    MissingHomeDirectory,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml serialization error: {0}")]
    SerializeToml(#[from] toml::ser::Error),
    #[error("toml deserialization error: {0}")]
    DeserializeToml(#[from] toml::de::Error),
    #[error("corpus already exists: {0}")]
    CorpusAlreadyExists(String),
    #[error("corpus not found: {0}")]
    CorpusNotFound(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePaths {
    pub state_root: PathBuf,
    pub config_path: PathBuf,
}

impl RuntimePaths {
    pub fn from_state_root(state_root: PathBuf) -> Self {
        let config_path = state_root.join("config.toml");
        Self {
            state_root,
            config_path,
        }
    }

    pub fn from_env() -> Result<Self, RuntimeError> {
        if let Some(override_root) = env::var_os("NET_RGA_STATE_ROOT") {
            return Ok(Self::from_state_root(PathBuf::from(override_root)));
        }
        let home = env::var_os("HOME").ok_or(RuntimeError::MissingHomeDirectory)?;
        Ok(Self::from_state_root(StateLayout::default_state_root(Path::new(
            &PathBuf::from(home),
        ))))
    }
}

pub struct ConfigStore {
    paths: RuntimePaths,
}

impl ConfigStore {
    pub fn new(paths: RuntimePaths) -> Self {
        Self { paths }
    }

    pub fn load(&self) -> Result<AppConfig, RuntimeError> {
        if !self.paths.config_path.exists() {
            return Ok(AppConfig::default());
        }
        let content = fs::read_to_string(&self.paths.config_path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save(&self, config: &AppConfig) -> Result<(), RuntimeError> {
        fs::create_dir_all(&self.paths.state_root)?;
        let encoded = toml::to_string_pretty(config)?;
        fs::write(&self.paths.config_path, encoded)?;
        Ok(())
    }

    pub fn add_corpus(&self, corpus: CorpusConfig) -> Result<(), RuntimeError> {
        let mut config = self.load()?;
        if config.corpora.iter().any(|existing| existing.id == corpus.id) {
            return Err(RuntimeError::CorpusAlreadyExists(corpus.id));
        }
        config.corpora.push(corpus);
        self.save(&config)
    }

    pub fn remove_corpus(&self, corpus_id: &str) -> Result<(), RuntimeError> {
        let mut config = self.load()?;
        let original_count = config.corpora.len();
        config.corpora.retain(|corpus| corpus.id != corpus_id);
        if config.corpora.len() == original_count {
            return Err(RuntimeError::CorpusNotFound(corpus_id.to_owned()));
        }
        self.save(&config)
    }

    pub fn list_corpora(&self) -> Result<Vec<CorpusConfig>, RuntimeError> {
        Ok(self.load()?.corpora)
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{ConfigStore, RuntimePaths};
    use crate::config::{CorpusConfig, ProviderConfig};

    fn temp_state_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir().join("net-rga-config-tests").join(format!("state-{nanos}"))
    }

    #[test]
    fn config_store_persists_and_lists_corpora() {
        let state_root = temp_state_root();
        let store = ConfigStore::new(RuntimePaths::from_state_root(state_root.clone()));

        store
            .add_corpus(CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: PathBuf::from("/data"),
                },
                include_globs: vec!["docs/**".to_owned()],
                exclude_globs: Vec::new(),
                backend: None,
            })
            .unwrap_or_else(|error| panic!("corpus should save: {error}"));

        let corpora = store
            .list_corpora()
            .unwrap_or_else(|error| panic!("corpora should load: {error}"));
        assert_eq!(corpora.len(), 1);
        assert_eq!(corpora[0].id, "local");

        fs::remove_dir_all(state_root).ok();
    }

    #[test]
    fn removing_missing_corpus_returns_error() {
        let state_root = temp_state_root();
        let store = ConfigStore::new(RuntimePaths::from_state_root(state_root.clone()));

        let result = store.remove_corpus("missing");
        assert!(result.is_err());

        fs::remove_dir_all(state_root).ok();
    }
}

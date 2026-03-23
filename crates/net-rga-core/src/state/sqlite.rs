use std::fs;
use std::path::Path;

use rusqlite::Connection;
use thiserror::Error;

use crate::config::CorpusConfig;
use crate::state::MANIFEST_SCHEMA_V1;

const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct ManifestDb {
    connection: Connection,
}

impl ManifestDb {
    pub fn open(path: &Path) -> Result<Self, ManifestError> {
        let connection = open_manifest_db(path)?;
        Ok(Self { connection })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn upsert_corpus(
        &self,
        corpus: &CorpusConfig,
        provider_kind: &str,
        root: &str,
        config_toml: &str,
        timestamp: &str,
    ) -> Result<(), ManifestError> {
        self.connection.execute(
            "INSERT INTO corpora (id, display_name, provider_kind, root, config_toml, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET
                 display_name = excluded.display_name,
                 provider_kind = excluded.provider_kind,
                 root = excluded.root,
                 config_toml = excluded.config_toml,
                 updated_at = excluded.updated_at",
            rusqlite::params![
                corpus.id,
                corpus.display_name,
                provider_kind,
                root,
                config_toml,
                timestamp
            ],
        )?;
        Ok(())
    }

    pub fn upsert_sync_checkpoint(
        &self,
        corpus_id: &str,
        checkpoint_name: &str,
        cursor: Option<&str>,
        updated_at: &str,
    ) -> Result<(), ManifestError> {
        self.connection.execute(
            "INSERT INTO sync_checkpoints (corpus_id, checkpoint_name, cursor, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(corpus_id, checkpoint_name) DO UPDATE SET
                 cursor = excluded.cursor,
                 updated_at = excluded.updated_at",
            rusqlite::params![corpus_id, checkpoint_name, cursor, updated_at],
        )?;
        Ok(())
    }

    pub fn sync_checkpoint(
        &self,
        corpus_id: &str,
        checkpoint_name: &str,
    ) -> Result<Option<String>, ManifestError> {
        let mut statement = self.connection.prepare(
            "SELECT cursor FROM sync_checkpoints WHERE corpus_id = ?1 AND checkpoint_name = ?2",
        )?;
        let checkpoint = statement.query_row([corpus_id, checkpoint_name], |row| row.get(0));
        match checkpoint {
            Ok(value) => Ok(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(ManifestError::Sqlite(error)),
        }
    }
}

pub fn open_manifest_db(path: &Path) -> Result<Connection, ManifestError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path)?;
    apply_manifest_migrations(&connection)?;
    Ok(connection)
}

pub fn apply_manifest_migrations(connection: &Connection) -> Result<(), ManifestError> {
    let current_version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current_version < MANIFEST_SCHEMA_VERSION {
        connection.execute_batch(MANIFEST_SCHEMA_V1)?;
        connection.pragma_update(None, "user_version", MANIFEST_SCHEMA_VERSION)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{ManifestDb, open_manifest_db};
    use crate::config::{CorpusConfig, ProviderConfig};

    fn temp_manifest_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir()
            .join("net-rga-tests")
            .join(format!("manifest-{nanos}.db"))
    }

    #[test]
    fn migration_runner_creates_manifest_db_with_expected_version() {
        let path = temp_manifest_path();
        {
            let connection =
                open_manifest_db(&path).unwrap_or_else(|error| panic!("manifest should open: {error}"));
            let version: u32 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap_or_else(|error| panic!("pragma should read: {error}"));

            assert_eq!(version, 1);
        }
        fs::remove_file(path).ok();
    }

    #[test]
    fn manifest_db_exposes_live_connection() {
        let path = temp_manifest_path();
        {
            let manifest =
                ManifestDb::open(&path).unwrap_or_else(|error| panic!("manifest should open: {error}"));
            let table_count: u32 = manifest
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'documents'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|error| panic!("table count should query: {error}"));

            assert_eq!(table_count, 1);
        }
        fs::remove_file(path).ok();
    }

    #[test]
    fn manifest_db_persists_sync_checkpoints() {
        let path = temp_manifest_path();
        {
            let manifest =
                ManifestDb::open(&path).unwrap_or_else(|error| panic!("manifest should open: {error}"));
            let corpus = CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: PathBuf::from("/tmp/docs"),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            };
            manifest
                .upsert_corpus(&corpus, "local_fs", "/tmp/docs", "id = 'local'", "1000")
                .unwrap_or_else(|error| panic!("corpus should persist: {error}"));
            manifest
                .upsert_sync_checkpoint("local", "list_cursor", Some("abc123"), "1000")
                .unwrap_or_else(|error| panic!("checkpoint should persist: {error}"));

            let checkpoint = manifest
                .sync_checkpoint("local", "list_cursor")
                .unwrap_or_else(|error| panic!("checkpoint should load: {error}"));

            assert_eq!(checkpoint.as_deref(), Some("abc123"));
        }
        fs::remove_file(path).ok();
    }

    #[test]
    fn manifest_db_upserts_corpus_metadata() {
        let path = temp_manifest_path();
        {
            let manifest =
                ManifestDb::open(&path).unwrap_or_else(|error| panic!("manifest should open: {error}"));
            let corpus = CorpusConfig {
                id: "local".to_owned(),
                display_name: Some("Local".to_owned()),
                provider: ProviderConfig::LocalFs {
                    root: PathBuf::from("/tmp/docs"),
                },
                include_globs: Vec::new(),
                exclude_globs: Vec::new(),
                backend: None,
            };

            manifest
                .upsert_corpus(&corpus, "local_fs", "/tmp/docs", "id = 'local'", "1000")
                .unwrap_or_else(|error| panic!("corpus should persist: {error}"));

            let root: String = manifest
                .connection()
                .query_row("SELECT root FROM corpora WHERE id = 'local'", [], |row| row.get(0))
                .unwrap_or_else(|error| panic!("corpus root should query: {error}"));

            assert_eq!(root, "/tmp/docs");
        }
        fs::remove_file(path).ok();
    }
}

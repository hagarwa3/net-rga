use std::fs;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;

use crate::config::CorpusConfig;
use crate::domain::DocumentMeta;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentUpsertStatus {
    Inserted,
    Updated,
    Unchanged,
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

    pub fn upsert_document(
        &self,
        corpus_id: &str,
        document: &DocumentMeta,
        last_seen_at: &str,
    ) -> Result<DocumentUpsertStatus, ManifestError> {
        let existing: Option<(Option<String>, String, u64, Option<String>)> = self
            .connection
            .query_row(
                "SELECT version, path, size_bytes, modified_at
                 FROM documents
                 WHERE corpus_id = ?1 AND document_id = ?2",
                rusqlite::params![corpus_id, document.id.0],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        self.connection.execute(
            "INSERT INTO documents (
                 corpus_id,
                 document_id,
                 path,
                 extension,
                 content_type,
                 version,
                 size_bytes,
                 modified_at,
                 last_seen_at,
                 extraction_status,
                 index_status,
                 cache_status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', 'pending', 'pending')
             ON CONFLICT(corpus_id, document_id) DO UPDATE SET
                 path = excluded.path,
                 extension = excluded.extension,
                 content_type = excluded.content_type,
                 version = excluded.version,
                 size_bytes = excluded.size_bytes,
                 modified_at = excluded.modified_at,
                 last_seen_at = excluded.last_seen_at,
                 extraction_status = CASE
                     WHEN COALESCE(documents.version, '') <> COALESCE(excluded.version, '')
                         OR documents.size_bytes <> excluded.size_bytes
                         OR COALESCE(documents.modified_at, '') <> COALESCE(excluded.modified_at, '')
                     THEN 'pending'
                     ELSE documents.extraction_status
                 END,
                 index_status = CASE
                     WHEN COALESCE(documents.version, '') <> COALESCE(excluded.version, '')
                         OR documents.size_bytes <> excluded.size_bytes
                         OR COALESCE(documents.modified_at, '') <> COALESCE(excluded.modified_at, '')
                     THEN 'pending'
                     ELSE documents.index_status
                 END,
                 cache_status = CASE
                     WHEN COALESCE(documents.version, '') <> COALESCE(excluded.version, '')
                         OR documents.size_bytes <> excluded.size_bytes
                         OR COALESCE(documents.modified_at, '') <> COALESCE(excluded.modified_at, '')
                     THEN 'pending'
                     ELSE documents.cache_status
                 END",
            rusqlite::params![
                corpus_id,
                document.id.0,
                document.locator.path,
                document.extension,
                document.content_type,
                document.version,
                document.size_bytes,
                document.modified_at,
                last_seen_at
            ],
        )?;
        self.connection.execute(
            "DELETE FROM tombstones WHERE corpus_id = ?1 AND document_id = ?2",
            rusqlite::params![corpus_id, document.id.0],
        )?;

        Ok(match existing {
            None => DocumentUpsertStatus::Inserted,
            Some((version, path, size_bytes, modified_at))
                if version == document.version
                    && path == document.locator.path
                    && size_bytes == document.size_bytes
                    && modified_at == document.modified_at =>
            {
                DocumentUpsertStatus::Unchanged
            }
            Some(_) => DocumentUpsertStatus::Updated,
        })
    }

    pub fn document_count(&self, corpus_id: &str) -> Result<u64, ManifestError> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM documents WHERE corpus_id = ?1",
                [corpus_id],
                |row| row.get(0),
            )
            .map_err(ManifestError::Sqlite)
    }

    pub fn tombstone_missing_documents(
        &self,
        corpus_id: &str,
        active_last_seen_at: &str,
        deleted_at: &str,
    ) -> Result<u64, ManifestError> {
        let deleted_count = self.connection.execute(
            "INSERT OR REPLACE INTO tombstones (corpus_id, document_id, path, version, deleted_at)
             SELECT corpus_id, document_id, path, version, ?3
             FROM documents
             WHERE corpus_id = ?1 AND last_seen_at <> ?2",
            rusqlite::params![corpus_id, active_last_seen_at, deleted_at],
        )?;
        self.connection.execute(
            "DELETE FROM documents
             WHERE corpus_id = ?1 AND last_seen_at <> ?2",
            rusqlite::params![corpus_id, active_last_seen_at],
        )?;
        Ok(u64::try_from(deleted_count).unwrap_or_default())
    }

    pub fn tombstone_count(&self, corpus_id: &str) -> Result<u64, ManifestError> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM tombstones WHERE corpus_id = ?1",
                [corpus_id],
                |row| row.get(0),
            )
            .map_err(ManifestError::Sqlite)
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{DocumentUpsertStatus, ManifestDb, open_manifest_db};
    use crate::config::{CorpusConfig, ProviderConfig};
    use crate::domain::{DocumentId, DocumentLocator, DocumentMeta};

    fn temp_manifest_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir()
            .join("net-rga-tests")
            .join(format!("manifest-{nanos}-{sequence}"))
            .join("manifest.db")
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

    #[test]
    fn manifest_db_upserts_document_metadata() {
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

            let inserted = manifest
                .upsert_document(
                    "local",
                    &DocumentMeta {
                        id: DocumentId("docs/report.txt".to_owned()),
                        locator: DocumentLocator {
                            path: "docs/report.txt".to_owned(),
                        },
                        extension: Some("txt".to_owned()),
                        content_type: Some("text/plain".to_owned()),
                        version: Some("v1".to_owned()),
                        size_bytes: 10,
                        modified_at: Some("1000".to_owned()),
                    },
                    "1000",
                )
                .unwrap_or_else(|error| panic!("document should upsert: {error}"));
            let updated = manifest
                .upsert_document(
                    "local",
                    &DocumentMeta {
                        id: DocumentId("docs/report.txt".to_owned()),
                        locator: DocumentLocator {
                            path: "docs/report.txt".to_owned(),
                        },
                        extension: Some("txt".to_owned()),
                        content_type: Some("text/plain".to_owned()),
                        version: Some("v2".to_owned()),
                        size_bytes: 20,
                        modified_at: Some("2000".to_owned()),
                    },
                    "2000",
                )
                .unwrap_or_else(|error| panic!("document should update: {error}"));

            assert_eq!(inserted, DocumentUpsertStatus::Inserted);
            assert_eq!(updated, DocumentUpsertStatus::Updated);
            assert_eq!(
                manifest
                    .document_count("local")
                    .unwrap_or_else(|error| panic!("document count should query: {error}")),
                1
            );
        }
        fs::remove_file(path).ok();
    }

    #[test]
    fn manifest_db_tombstones_missing_documents() {
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
                .upsert_document(
                    "local",
                    &DocumentMeta {
                        id: DocumentId("docs/report.txt".to_owned()),
                        locator: DocumentLocator {
                            path: "docs/report.txt".to_owned(),
                        },
                        extension: Some("txt".to_owned()),
                        content_type: Some("text/plain".to_owned()),
                        version: Some("v1".to_owned()),
                        size_bytes: 10,
                        modified_at: Some("1000".to_owned()),
                    },
                    "1000",
                )
                .unwrap_or_else(|error| panic!("document should upsert: {error}"));

            let tombstoned = manifest
                .tombstone_missing_documents("local", "2000", "2000")
                .unwrap_or_else(|error| panic!("document should tombstone: {error}"));

            assert_eq!(tombstoned, 1);
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
        }
        fs::remove_file(path).ok();
    }
}

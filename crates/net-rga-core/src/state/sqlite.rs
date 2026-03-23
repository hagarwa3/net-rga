use std::fs;
use std::path::Path;

use rusqlite::Connection;
use thiserror::Error;

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
        let connection = open_manifest_db(&path).unwrap_or_else(|error| panic!("manifest should open: {error}"));
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap_or_else(|error| panic!("pragma should read: {error}"));

        assert_eq!(version, 1);
        fs::remove_file(path).ok();
    }

    #[test]
    fn manifest_db_exposes_live_connection() {
        let path = temp_manifest_path();
        let manifest = ManifestDb::open(&path).unwrap_or_else(|error| panic!("manifest should open: {error}"));
        let table_count: u32 = manifest
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'documents'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| panic!("table count should query: {error}"));

        assert_eq!(table_count, 1);
        fs::remove_file(path).ok();
    }
}


use std::collections::HashMap;
use std::fs;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::{CanonicalDocument, DocumentMeta};

pub const INDEX_SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS index_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS indexed_documents (
    document_id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    version TEXT,
    content_kind TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS indexed_chunks USING fts5 (
    document_id UNINDEXED,
    path UNINDEXED,
    anchor_ref UNINDEXED,
    snippet
);
"#;

const INDEX_SCHEMA_VERSION: &str = "1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexUpdateStrategy {
    ManualBuild,
}

impl IndexUpdateStrategy {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ManualBuild => "manual_build",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexedChunkHit {
    pub document_id: String,
    pub path: String,
    pub anchor_ref: String,
    pub snippet: String,
    pub score: f64,
}

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct LexicalIndex {
    connection: Connection,
}

impl LexicalIndex {
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(INDEX_SCHEMA_V1)?;
        let index = Self { connection };
        index.write_health_metadata("schema_version", INDEX_SCHEMA_VERSION)?;
        index
            .write_health_metadata("update_strategy", IndexUpdateStrategy::ManualBuild.as_str())?;
        Ok(index)
    }

    pub fn open_read_only(path: &Path) -> Result<Self, IndexError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<Option<String>, IndexError> {
        self.read_health_metadata("schema_version")
    }

    pub fn update_strategy(&self) -> Result<Option<String>, IndexError> {
        self.read_health_metadata("update_strategy")
    }

    pub fn upsert_document(
        &self,
        meta: &DocumentMeta,
        canonical: &CanonicalDocument,
    ) -> Result<(), IndexError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM indexed_documents WHERE document_id = ?1",
            params![meta.id.0],
        )?;
        transaction.execute(
            "DELETE FROM indexed_chunks WHERE document_id = ?1",
            params![meta.id.0],
        )?;
        transaction.execute(
            "INSERT INTO indexed_documents (document_id, path, version, content_kind, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                meta.id.0,
                meta.locator.path,
                meta.version,
                format!("{:?}", canonical.content_kind).to_ascii_lowercase(),
                meta.modified_at.clone().unwrap_or_default(),
            ],
        )?;
        for chunk in &canonical.chunks {
            transaction.execute(
                "INSERT INTO indexed_chunks (document_id, path, anchor_ref, snippet)
                 VALUES (?1, ?2, ?3, ?4)",
                params![meta.id.0, meta.locator.path, chunk.anchor_ref, chunk.text,],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn remove_document(&self, document_id: &str) -> Result<(), IndexError> {
        self.connection.execute(
            "DELETE FROM indexed_documents WHERE document_id = ?1",
            params![document_id],
        )?;
        self.connection.execute(
            "DELETE FROM indexed_chunks WHERE document_id = ?1",
            params![document_id],
        )?;
        Ok(())
    }

    pub fn reconcile_manifest(
        &self,
        manifest_documents: &[DocumentMeta],
    ) -> Result<u64, IndexError> {
        let manifest_versions = manifest_documents
            .iter()
            .map(|document| (document.id.0.clone(), document.version.clone()))
            .collect::<HashMap<_, _>>();

        let mut statement = self
            .connection
            .prepare("SELECT document_id, version FROM indexed_documents")?;
        let indexed_rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;

        let mut removed = 0_u64;
        for row in indexed_rows {
            let (document_id, version) = row?;
            let manifest_version = manifest_versions.get(&document_id);
            if manifest_version.is_none() || manifest_version != Some(&version) {
                self.remove_document(&document_id)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn indexed_document_count(&self) -> Result<u64, IndexError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM indexed_documents", [], |row| {
                row.get(0)
            })?)
    }

    pub fn query_fixed_string(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<IndexedChunkHit>, IndexError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let phrase = format!("\"{}\"", query.replace('"', "\"\""));
        let mut statement = self.connection.prepare(
            "SELECT document_id, path, anchor_ref, snippet, bm25(indexed_chunks)
             FROM indexed_chunks
             WHERE indexed_chunks MATCH ?1
             ORDER BY bm25(indexed_chunks)
             LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![phrase, i64::try_from(limit).unwrap_or(i64::MAX)],
            |row| {
                Ok(IndexedChunkHit {
                    document_id: row.get(0)?,
                    path: row.get(1)?,
                    anchor_ref: row.get(2)?,
                    snippet: row.get(3)?,
                    score: row.get(4)?,
                })
            },
        )?;

        let mut hits = Vec::new();
        for row in rows {
            hits.push(row?);
        }
        Ok(hits)
    }

    pub fn read_health_metadata(&self, key: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM index_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn write_health_metadata(&self, key: &str, value: &str) -> Result<(), IndexError> {
        self.connection.execute(
            "INSERT INTO index_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::{
        Anchor, AnchorKind, AnchorLocator, CanonicalChunk, CanonicalContentKind, CanonicalDocument,
        DocumentId, DocumentLocator, DocumentMeta,
    };

    use super::{INDEX_SCHEMA_VERSION, IndexUpdateStrategy, LexicalIndex};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_index_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir()
            .join("net-rga-index-tests")
            .join(format!("index-{}-{nanos}-{counter}.db", std::process::id()))
    }

    #[test]
    fn lexical_index_persists_schema_and_update_strategy() {
        let path = temp_index_path();
        let index =
            LexicalIndex::open(&path).unwrap_or_else(|error| panic!("index should open: {error}"));

        assert_eq!(
            index.schema_version().unwrap_or_default().as_deref(),
            Some(INDEX_SCHEMA_VERSION)
        );
        assert_eq!(
            index.update_strategy().unwrap_or_default().as_deref(),
            Some(IndexUpdateStrategy::ManualBuild.as_str())
        );

        fs::remove_file(path).ok();
    }

    #[test]
    fn lexical_index_upserts_and_queries_verified_chunks() {
        let path = temp_index_path();
        let index =
            LexicalIndex::open(&path).unwrap_or_else(|error| panic!("index should open: {error}"));
        let meta = document_meta("docs/report.txt", Some("v1"));
        let canonical = canonical_document("docs/report.txt", "riverglass appears here");

        index
            .upsert_document(&meta, &canonical)
            .unwrap_or_else(|error| panic!("document should index: {error}"));
        let hits = index
            .query_fixed_string("riverglass", 5)
            .unwrap_or_else(|error| panic!("query should succeed: {error}"));

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].document_id, "docs/report.txt");
        assert!(hits[0].snippet.contains("riverglass"));

        fs::remove_file(path).ok();
    }

    #[test]
    fn lexical_index_reconciles_stale_versions_against_manifest() {
        let path = temp_index_path();
        let index =
            LexicalIndex::open(&path).unwrap_or_else(|error| panic!("index should open: {error}"));
        let meta = document_meta("docs/report.txt", Some("v1"));
        let canonical = canonical_document("docs/report.txt", "riverglass appears here");
        index
            .upsert_document(&meta, &canonical)
            .unwrap_or_else(|error| panic!("document should index: {error}"));

        let removed = index
            .reconcile_manifest(&[document_meta("docs/report.txt", Some("v2"))])
            .unwrap_or_else(|error| panic!("reconcile should succeed: {error}"));
        let hits = index
            .query_fixed_string("riverglass", 5)
            .unwrap_or_else(|error| panic!("query should succeed: {error}"));

        assert_eq!(removed, 1);
        assert!(hits.is_empty());

        fs::remove_file(path).ok();
    }

    fn document_meta(path: &str, version: Option<&str>) -> DocumentMeta {
        DocumentMeta {
            id: DocumentId(path.to_owned()),
            locator: DocumentLocator {
                path: path.to_owned(),
            },
            extension: Some("txt".to_owned()),
            content_type: Some("text/plain".to_owned()),
            version: version.map(ToOwned::to_owned),
            size_bytes: 0,
            modified_at: Some("100".to_owned()),
        }
    }

    fn canonical_document(path: &str, snippet: &str) -> CanonicalDocument {
        let anchor = Anchor {
            kind: AnchorKind::LineSpan,
            locator: AnchorLocator {
                path: Some(path.to_owned()),
                line_start: Some(1),
                line_end: Some(1),
                ..AnchorLocator::default()
            },
        };
        CanonicalDocument {
            document_id: DocumentId(path.to_owned()),
            locator: DocumentLocator {
                path: path.to_owned(),
            },
            content_kind: CanonicalContentKind::Text,
            text: snippet.to_owned(),
            chunks: vec![CanonicalChunk {
                anchor_ref: anchor.stable_ref(),
                anchor,
                text: snippet.to_owned(),
            }],
            warnings: Vec::new(),
        }
    }
}

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS corpora (
    id TEXT PRIMARY KEY,
    display_name TEXT,
    provider_kind TEXT NOT NULL,
    root TEXT NOT NULL,
    config_toml TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS documents (
    corpus_id TEXT NOT NULL,
    document_id TEXT NOT NULL,
    path TEXT NOT NULL,
    extension TEXT,
    content_type TEXT,
    version TEXT,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    modified_at TEXT,
    last_seen_at TEXT NOT NULL,
    extraction_status TEXT NOT NULL DEFAULT 'pending',
    index_status TEXT NOT NULL DEFAULT 'pending',
    cache_status TEXT NOT NULL DEFAULT 'pending',
    PRIMARY KEY (corpus_id, document_id),
    FOREIGN KEY (corpus_id) REFERENCES corpora(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_documents_corpus_path
ON documents (corpus_id, path);

CREATE INDEX IF NOT EXISTS idx_documents_corpus_last_seen
ON documents (corpus_id, last_seen_at);

CREATE TABLE IF NOT EXISTS sync_checkpoints (
    corpus_id TEXT NOT NULL,
    checkpoint_name TEXT NOT NULL,
    cursor TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (corpus_id, checkpoint_name),
    FOREIGN KEY (corpus_id) REFERENCES corpora(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tombstones (
    corpus_id TEXT NOT NULL,
    document_id TEXT NOT NULL,
    path TEXT NOT NULL,
    version TEXT,
    deleted_at TEXT NOT NULL,
    PRIMARY KEY (corpus_id, document_id),
    FOREIGN KEY (corpus_id) REFERENCES corpora(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tombstones_corpus_deleted_at
ON tombstones (corpus_id, deleted_at);

CREATE TABLE IF NOT EXISTS failure_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    corpus_id TEXT NOT NULL,
    document_id TEXT,
    phase TEXT NOT NULL,
    error_kind TEXT NOT NULL,
    message TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    FOREIGN KEY (corpus_id) REFERENCES corpora(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_failure_records_corpus_recorded_at
ON failure_records (corpus_id, recorded_at);


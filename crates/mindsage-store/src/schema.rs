//! Database schema SQL — matches the Python SQLiteStore exactly.

/// Core tables: documents, chunks, chunk_embeddings.
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL,
    metadata_json TEXT,
    content_hash TEXT UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER
);

CREATE TABLE IF NOT EXISTS chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    parent_chunk_id INTEGER REFERENCES chunks(id),
    text TEXT NOT NULL,
    enriched_text TEXT,
    chunk_index INTEGER NOT NULL,
    char_start INTEGER,
    char_end INTEGER,
    level INTEGER DEFAULT 0,
    metadata_json TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chunks_doc_id ON chunks(doc_id);
CREATE INDEX IF NOT EXISTS idx_chunks_level ON chunks(level);
CREATE INDEX IF NOT EXISTS idx_chunks_parent ON chunks(parent_chunk_id);
CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(content_hash);

CREATE TABLE IF NOT EXISTS chunk_embeddings (
    chunk_id INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
    embedding BLOB NOT NULL,
    scale REAL NOT NULL,
    offset_val REAL NOT NULL
);
"#;

/// FTS5 virtual table for full-text search.
pub const FTS_SCHEMA_SQL: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    text, enriched_text,
    content='chunks', content_rowid='id',
    tokenize='porter unicode61'
);
"#;

/// Triggers to keep FTS index in sync with chunks table.
pub const FTS_TRIGGERS_SQL: &str = r#"
CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, text, enriched_text)
    VALUES (new.id, new.text, COALESCE(new.enriched_text, ''));
END;

CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text, enriched_text)
    VALUES ('delete', old.id, old.text, COALESCE(old.enriched_text, ''));
END;

CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text, enriched_text)
    VALUES ('delete', old.id, old.text, COALESCE(old.enriched_text, ''));
    INSERT INTO chunks_fts(rowid, text, enriched_text)
    VALUES (new.id, new.text, COALESCE(new.enriched_text, ''));
END;
"#;

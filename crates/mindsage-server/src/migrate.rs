//! Data migration tool — validates and imports data from existing Python installation.
//!
//! The Python and Rust backends share the same SQLite schema, so the database
//! is directly compatible. This module handles:
//! - Schema validation (verifies tables/columns match)
//! - Path adjustment in `.indexed-files.json`
//! - LLM config migration
//! - Browser connector state migration

use std::path::Path;

use rusqlite::Connection;
use tracing::{error, info};

/// Result of a migration check or operation.
#[derive(Debug)]
pub struct MigrationReport {
    pub db_valid: bool,
    pub documents: i64,
    pub chunks: i64,
    pub embeddings: i64,
    pub indexed_files_migrated: usize,
    pub llm_config_migrated: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Validate that a data directory contains a compatible MindSage database.
pub fn validate(data_dir: &Path) -> MigrationReport {
    let mut report = MigrationReport {
        db_valid: false,
        documents: 0,
        chunks: 0,
        embeddings: 0,
        indexed_files_migrated: 0,
        llm_config_migrated: false,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    let db_path = data_dir.join("vectordb/mindsage.db");
    if !db_path.exists() {
        report.errors.push(format!(
            "Database not found: {}",
            db_path.display()
        ));
        return report;
    }

    // Open and validate schema
    let conn = match Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(c) => c,
        Err(e) => {
            report.errors.push(format!("Failed to open database: {}", e));
            return report;
        }
    };

    // Check required tables exist
    let required_tables = ["documents", "chunks", "chunk_embeddings", "chunks_fts"];
    for table in &required_tables {
        match table_exists(&conn, table) {
            Ok(true) => {}
            Ok(false) => {
                report.errors.push(format!("Missing required table: {}", table));
            }
            Err(e) => {
                report.errors.push(format!("Error checking table {}: {}", table, e));
            }
        }
    }

    if !report.errors.is_empty() {
        return report;
    }

    // Validate documents table columns
    let doc_columns = get_column_names(&conn, "documents");
    let required_doc_cols = ["id", "text", "metadata_json", "content_hash", "created_at"];
    for col in &required_doc_cols {
        if !doc_columns.contains(&col.to_string()) {
            report.errors.push(format!(
                "documents table missing column: {}",
                col
            ));
        }
    }

    // Validate chunks table columns
    let chunk_columns = get_column_names(&conn, "chunks");
    let required_chunk_cols = [
        "id", "doc_id", "parent_chunk_id", "text", "enriched_text",
        "chunk_index", "char_start", "char_end", "level", "created_at",
    ];
    for col in &required_chunk_cols {
        if !chunk_columns.contains(&col.to_string()) {
            report.errors.push(format!(
                "chunks table missing column: {}",
                col
            ));
        }
    }

    // Validate chunk_embeddings table
    let emb_columns = get_column_names(&conn, "chunk_embeddings");
    let required_emb_cols = ["chunk_id", "embedding", "scale", "offset_val"];
    for col in &required_emb_cols {
        if !emb_columns.contains(&col.to_string()) {
            report.errors.push(format!(
                "chunk_embeddings table missing column: {}",
                col
            ));
        }
    }

    if !report.errors.is_empty() {
        return report;
    }

    report.db_valid = true;

    // Gather statistics
    report.documents = count_rows(&conn, "documents").unwrap_or(0);
    report.chunks = count_rows(&conn, "chunks").unwrap_or(0);
    report.embeddings = count_rows(&conn, "chunk_embeddings").unwrap_or(0);

    // Check embedding dimension
    if report.embeddings > 0 {
        if let Ok(dim) = conn.query_row(
            "SELECT length(embedding) FROM chunk_embeddings LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        ) {
            if dim != 384 {
                report.warnings.push(format!(
                    "Unexpected embedding dimension: {} (expected 384)",
                    dim
                ));
            }
        }
    }

    // Check for orphaned chunks
    if let Ok(orphans) = conn.query_row(
        "SELECT COUNT(*) FROM chunks WHERE doc_id NOT IN (SELECT id FROM documents)",
        [],
        |row| row.get::<_, i64>(0),
    ) {
        if orphans > 0 {
            report.warnings.push(format!("{} orphaned chunks found", orphans));
        }
    }

    // Check ancillary files
    let llm_config = data_dir.join("llm-config.json");
    if llm_config.exists() {
        report.llm_config_migrated = true;
    } else {
        report.warnings.push("No llm-config.json found".to_string());
    }

    let indexed_files = data_dir.join(".indexed-files.json");
    if indexed_files.exists() {
        match std::fs::read_to_string(&indexed_files) {
            Ok(content) => {
                if let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content) {
                    report.indexed_files_migrated = map.len();
                }
            }
            Err(e) => {
                report.warnings.push(format!("Cannot read .indexed-files.json: {}", e));
            }
        }
    }

    // Check for legacy ObjectBox files
    let objectbox_data = data_dir.join("vectordb/data.mdb");
    if objectbox_data.exists() {
        report.warnings.push(
            "Legacy ObjectBox files found (data.mdb). Safe to delete after migration.".to_string(),
        );
    }

    report
}

/// Migrate file paths in .indexed-files.json to use the new data directory.
///
/// The Python backend may have used /app/data/ (Docker) paths, while the
/// Rust binary uses relative or different absolute paths.
pub fn migrate_indexed_files(data_dir: &Path, new_data_dir: &Path) -> Result<usize, String> {
    let src = data_dir.join(".indexed-files.json");
    if !src.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(&src)
        .map_err(|e| format!("Failed to read .indexed-files.json: {}", e))?;

    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid .indexed-files.json: {}", e))?;

    let old_prefix = data_dir.to_string_lossy();
    let new_prefix = new_data_dir.to_string_lossy();

    let mut new_map = serde_json::Map::new();
    let mut count = 0;

    for (key, mut value) in map {
        // Update the key (file path)
        let new_key = key.replace(old_prefix.as_ref(), new_prefix.as_ref());

        // Update filePath inside the value
        if let Some(obj) = value.as_object_mut() {
            if let Some(fp) = obj.get("filePath").and_then(|v| v.as_str()) {
                let new_fp = fp.replace(old_prefix.as_ref(), new_prefix.as_ref());
                obj.insert("filePath".to_string(), serde_json::json!(new_fp));
            }
        }

        new_map.insert(new_key, value);
        count += 1;
    }

    let dst = new_data_dir.join(".indexed-files.json");
    let output = serde_json::to_string_pretty(&new_map)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    std::fs::write(&dst, output)
        .map_err(|e| format!("Failed to write {}: {}", dst.display(), e))?;

    Ok(count)
}

/// Run the full migration: validate source, copy DB and state files.
pub fn run_migration(source_dir: &Path, target_dir: &Path) -> MigrationReport {
    info!("Starting migration: {} → {}", source_dir.display(), target_dir.display());

    let mut report = validate(source_dir);
    if !report.db_valid {
        error!("Source database validation failed");
        return report;
    }

    info!(
        "Source validated: {} documents, {} chunks, {} embeddings",
        report.documents, report.chunks, report.embeddings
    );

    // Ensure target directories exist
    let target_vectordb = target_dir.join("vectordb");
    let target_uploads = target_dir.join("uploads");
    let target_imports = target_dir.join("imports");
    let target_exports = target_dir.join("exports");
    let target_browser = target_dir.join("browser-connector");

    for dir in [&target_vectordb, &target_uploads, &target_imports, &target_exports, &target_browser] {
        if let Err(e) = std::fs::create_dir_all(dir) {
            report.errors.push(format!("Failed to create {}: {}", dir.display(), e));
            return report;
        }
    }

    // Copy database file (not WAL — let SQLite rebuild it)
    let src_db = source_dir.join("vectordb/mindsage.db");
    let dst_db = target_vectordb.join("mindsage.db");
    if src_db != dst_db {
        if let Err(e) = std::fs::copy(&src_db, &dst_db) {
            report.errors.push(format!("Failed to copy database: {}", e));
            return report;
        }
        info!("Copied database to {}", dst_db.display());
    }

    // Copy LLM config
    let src_llm = source_dir.join("llm-config.json");
    let dst_llm = target_dir.join("llm-config.json");
    if src_llm.exists() && src_llm != dst_llm {
        if let Err(e) = std::fs::copy(&src_llm, &dst_llm) {
            report.warnings.push(format!("Failed to copy llm-config.json: {}", e));
        } else {
            report.llm_config_migrated = true;
            info!("Copied llm-config.json");
        }
    }

    // Migrate indexed files with path adjustment
    match migrate_indexed_files(source_dir, target_dir) {
        Ok(count) => {
            report.indexed_files_migrated = count;
            if count > 0 {
                info!("Migrated {} indexed file records", count);
            }
        }
        Err(e) => {
            report.warnings.push(format!("Failed to migrate indexed files: {}", e));
        }
    }

    // Copy browser connector state
    let src_captures = source_dir.join("browser-connector/captures");
    let dst_captures = target_browser.join("captures");
    if src_captures.exists() {
        if let Err(e) = std::fs::create_dir_all(&dst_captures) {
            report.warnings.push(format!("Failed to create captures dir: {}", e));
        } else if let Ok(entries) = std::fs::read_dir(&src_captures) {
            for entry in entries.flatten() {
                let dst = dst_captures.join(entry.file_name());
                let _ = std::fs::copy(entry.path(), dst);
            }
        }
    }

    // Copy import files
    let src_imports = source_dir.join("imports");
    if src_imports.exists() {
        if let Ok(entries) = std::fs::read_dir(&src_imports) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    let dst = target_imports.join(entry.file_name());
                    let _ = std::fs::copy(entry.path(), dst);
                }
            }
        }
    }

    info!("Migration complete");
    report
}

/// Print a migration report to stdout.
pub fn print_report(report: &MigrationReport) {
    println!("=== MindSage Migration Report ===");
    println!();
    println!("Database valid:     {}", if report.db_valid { "YES" } else { "NO" });
    println!("Documents:          {}", report.documents);
    println!("Chunks:             {}", report.chunks);
    println!("Embeddings:         {}", report.embeddings);
    println!("Indexed files:      {}", report.indexed_files_migrated);
    println!("LLM config:         {}", if report.llm_config_migrated { "migrated" } else { "not found" });

    if !report.warnings.is_empty() {
        println!();
        println!("Warnings:");
        for w in &report.warnings {
            println!("  - {}", w);
        }
    }

    if !report.errors.is_empty() {
        println!();
        println!("Errors:");
        for e in &report.errors {
            println!("  - {}", e);
        }
    }

    println!();
    if report.errors.is_empty() && report.db_valid {
        println!("Status: READY FOR USE");
    } else {
        println!("Status: MIGRATION FAILED");
    }
}

// Internal helpers

fn table_exists(conn: &Connection, table: &str) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn get_column_names(conn: &Connection, table: &str) -> Vec<String> {
    // Use PRAGMA table_info which is safe from injection since table name
    // is hardcoded from our required_tables list
    let query = format!("PRAGMA table_info({})", table);
    let mut names = Vec::new();
    if let Ok(mut stmt) = conn.prepare(&query) {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) {
            for name in rows.flatten() {
                names.push(name);
            }
        }
    }
    names
}

fn count_rows(conn: &Connection, table: &str) -> Result<i64, rusqlite::Error> {
    let query = format!("SELECT COUNT(*) FROM {}", table);
    conn.query_row(&query, [], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db(dir: &Path) {
        let db_dir = dir.join("vectordb");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db_path = db_dir.join("mindsage.db");
        let conn = Connection::open(&db_path).unwrap();

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE documents (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 text TEXT NOT NULL,
                 metadata_json TEXT,
                 content_hash TEXT UNIQUE,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER
             );
             CREATE TABLE chunks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 doc_id INTEGER NOT NULL REFERENCES documents(id),
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
             CREATE TABLE chunk_embeddings (
                 chunk_id INTEGER PRIMARY KEY REFERENCES chunks(id),
                 embedding BLOB NOT NULL,
                 scale REAL NOT NULL,
                 offset_val REAL NOT NULL
             );
             CREATE VIRTUAL TABLE chunks_fts USING fts5(text, enriched_text, content='chunks', content_rowid='id');
             INSERT INTO documents (text, content_hash, created_at) VALUES ('Hello world', 'abc123', 1000);
             INSERT INTO chunks (doc_id, text, chunk_index, level, created_at) VALUES (1, 'Hello world', 0, 1, 1000);",
        )
        .unwrap();
    }

    #[test]
    fn test_validate_valid_db() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_db(dir.path());

        let report = validate(dir.path());
        assert!(report.db_valid);
        assert_eq!(report.documents, 1);
        assert_eq!(report.chunks, 1);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_validate_missing_db() {
        let dir = tempfile::tempdir().unwrap();
        let report = validate(dir.path());
        assert!(!report.db_valid);
        assert!(!report.errors.is_empty());
    }

    #[test]
    fn test_migrate_indexed_files() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        let indexed = serde_json::json!({
            "/app/data/imports/test.txt": {
                "filename": "test.txt",
                "filePath": "/app/data/imports/test.txt",
                "indexedAt": "2026-01-01T00:00:00Z",
                "documentId": 1,
                "size": 100,
                "modified": "2026-01-01T00:00:00Z"
            }
        });
        let src_path = src.path().join(".indexed-files.json");
        std::fs::write(&src_path, serde_json::to_string(&indexed).unwrap()).unwrap();

        let count = migrate_indexed_files(src.path(), dst.path()).unwrap();
        assert_eq!(count, 1);

        let dst_path = dst.path().join(".indexed-files.json");
        assert!(dst_path.exists());
    }

    #[test]
    fn test_run_migration() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        setup_test_db(src.path());

        // Add llm-config.json
        std::fs::write(
            src.path().join("llm-config.json"),
            r#"{"preferredProvider":"auto"}"#,
        )
        .unwrap();

        let report = run_migration(src.path(), dst.path());
        assert!(report.db_valid);
        assert_eq!(report.documents, 1);
        assert!(report.llm_config_migrated);
        assert!(report.errors.is_empty());

        // Verify DB was copied
        assert!(dst.path().join("vectordb/mindsage.db").exists());
        assert!(dst.path().join("llm-config.json").exists());
    }
}

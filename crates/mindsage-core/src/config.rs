//! Configuration and data directory management.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Paths to all MindSage data directories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPaths {
    /// Root data directory (e.g., `data/`).
    pub root: PathBuf,
    /// Vector database directory (`data/vectordb/`).
    pub vectordb: PathBuf,
    /// File uploads directory (`data/uploads/`).
    pub uploads: PathBuf,
    /// Files queued for import (`data/imports/`).
    pub imports: PathBuf,
    /// Connector export data (`data/exports/`).
    pub exports: PathBuf,
    /// Connector configurations (`data/connectors.json`).
    pub connectors_file: PathBuf,
    /// Browser connector data (`data/browser-connector/`).
    pub browser_connector: PathBuf,
    /// LLM configuration (`data/llm-config.json`).
    pub llm_config_file: PathBuf,
    /// Indexed files tracking (`data/.indexed-files.json`).
    pub indexed_files: PathBuf,
}

impl DataPaths {
    /// Create data paths from a root directory. Creates directories if needed.
    pub fn new(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let paths = Self {
            vectordb: root.join("vectordb"),
            uploads: root.join("uploads"),
            imports: root.join("imports"),
            exports: root.join("exports"),
            connectors_file: root.join("connectors.json"),
            browser_connector: root.join("browser-connector"),
            llm_config_file: root.join("llm-config.json"),
            indexed_files: root.join(".indexed-files.json"),
            root,
        };
        paths.ensure_dirs()?;
        Ok(paths)
    }

    /// Create all required directories.
    fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.vectordb)?;
        std::fs::create_dir_all(&self.uploads)?;
        std::fs::create_dir_all(&self.imports)?;
        std::fs::create_dir_all(&self.exports)?;
        std::fs::create_dir_all(&self.browser_connector)?;
        Ok(())
    }
}

/// Top-level MindSage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MindSageConfig {
    /// HTTP server port.
    pub port: u16,
    /// Data directory paths.
    pub data_paths: DataPaths,
    /// Embedding dimension (384 for all-MiniLM-L6-v2).
    pub embedding_dim: usize,
}

impl MindSageConfig {
    /// Create configuration from environment and defaults.
    pub fn from_env(data_dir: impl AsRef<Path>) -> std::io::Result<Self> {
        let port = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3003);

        let data_paths = DataPaths::new(data_dir)?;

        Ok(Self {
            port,
            data_paths,
            embedding_dim: 384,
        })
    }
}

//! Shared application state.

use std::collections::HashMap;
use std::sync::Arc;
use mindsage_browser::BrowserManager;
use mindsage_chat::LLMConfig;
use mindsage_connectors::ConnectorManager;
use mindsage_core::MindSageConfig;
use mindsage_infer::EmbedderBackend;
use mindsage_localsend::LocalSendServer;
use mindsage_protocol::consent::ConsentManager;
use mindsage_protocol::pii::PiiDetector;
use mindsage_runtime::Orchestrator;
use mindsage_store::SqliteStore;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Indexing job status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingJob {
    pub id: String,
    pub filename: String,
    pub file_path: String,
    pub status: IndexingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub queued_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndexingStatus {
    Queued,
    Processing,
    Completed,
    Failed,
}

/// Indexed file tracking record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFileRecord {
    pub filename: String,
    pub file_path: String,
    pub indexed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<i64>,
    pub size: u64,
    pub modified: String,
}

/// Shared application state accessible from all route handlers.
pub struct AppState {
    pub config: MindSageConfig,
    pub store: SqliteStore,
    pub embedder: Arc<dyn EmbedderBackend>,
    pub llm_config: RwLock<LLMConfig>,
    pub browser_manager: BrowserManager,
    pub localsend_server: LocalSendServer,
    pub connector_manager: ConnectorManager,
    pub pii_detector: PiiDetector,
    pub consent_manager: ConsentManager,
    pub orchestrator: Orchestrator,
    pub indexing_jobs: RwLock<HashMap<String, IndexingJob>>,
    pub indexing_tx: mpsc::UnboundedSender<IndexingRequest>,
    indexing_rx: parking_lot::Mutex<Option<mpsc::UnboundedReceiver<IndexingRequest>>>,
    pub indexed_files: RwLock<HashMap<String, IndexedFileRecord>>,
}

/// A request to index a file.
pub struct IndexingRequest {
    pub job_id: String,
    pub file_path: String,
    pub filename: String,
}

impl AppState {
    pub fn new(config: MindSageConfig, store: SqliteStore, embedder: Arc<dyn EmbedderBackend>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Load indexed files from disk
        let indexed_files = Self::load_indexed_files(&config.data_paths.indexed_files);

        // Load LLM config
        let llm_config_path = config.data_paths.llm_config_file.clone();
        let llm_config = LLMConfig::load(&llm_config_path);

        // Initialize browser manager
        let browser_manager = BrowserManager::new(&config.data_paths.browser_connector);

        // Initialize LocalSend server
        let localsend_server = LocalSendServer::new(&config.data_paths.uploads, "MindSage");

        // Initialize connector manager
        let connector_manager = ConnectorManager::new(
            &config.data_paths.connectors_file,
            &config.data_paths.exports,
        );

        // Initialize privacy and runtime
        let pii_detector = PiiDetector::new();
        let consent_manager = ConsentManager::new();
        let orchestrator = Orchestrator::new();

        Self {
            config,
            store,
            embedder,
            llm_config: RwLock::new(llm_config),
            browser_manager,
            localsend_server,
            connector_manager,
            pii_detector,
            consent_manager,
            orchestrator,
            indexing_jobs: RwLock::new(HashMap::new()),
            indexing_tx: tx,
            indexing_rx: parking_lot::Mutex::new(Some(rx)),
            indexed_files: RwLock::new(indexed_files),
        }
    }

    /// Take the indexing receiver (can only be called once, by the worker).
    pub fn take_indexing_rx(&self) -> Option<mpsc::UnboundedReceiver<IndexingRequest>> {
        self.indexing_rx.lock().take()
    }

    fn load_indexed_files(
        path: &std::path::Path,
    ) -> HashMap<String, IndexedFileRecord> {
        match std::fs::read_to_string(path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    pub fn save_indexed_files(&self) {
        let indexed = self.indexed_files.read();
        if let Ok(data) = serde_json::to_string_pretty(&*indexed) {
            let _ = std::fs::write(&self.config.data_paths.indexed_files, data);
        }
    }

    pub fn is_file_indexed(&self, file_path: &str) -> bool {
        let indexed = self.indexed_files.read();
        if let Some(record) = indexed.get(file_path) {
            if let Ok(meta) = std::fs::metadata(file_path) {
                if let Ok(modified) = meta.modified() {
                    let modified_str = chrono::DateTime::<chrono::Utc>::from(modified)
                        .to_rfc3339();
                    return record.modified == modified_str;
                }
            }
        }
        false
    }

    pub fn mark_file_indexed(&self, file_path: &str, document_id: Option<i64>) {
        if let Ok(meta) = std::fs::metadata(file_path) {
            let modified_str = meta
                .modified()
                .ok()
                .map(|m| chrono::DateTime::<chrono::Utc>::from(m).to_rfc3339())
                .unwrap_or_default();

            let record = IndexedFileRecord {
                filename: std::path::Path::new(file_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string(),
                file_path: file_path.to_string(),
                indexed_at: chrono::Utc::now().to_rfc3339(),
                document_id,
                size: meta.len(),
                modified: modified_str,
            };

            self.indexed_files
                .write()
                .insert(file_path.to_string(), record);
            self.save_indexed_files();
        }
    }
}

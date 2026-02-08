# MindSage Rust Architecture

A single-binary Rust rewrite of the MindSage platform. Replaces the Express/TypeScript backend (port 3003) and Python/FastAPI vector store (port 8085) with one Axum server on port 3003. The React frontend works unchanged.

```
┌──────────────────────────────────────────────────────────────────────┐
│  React Frontend (port 8080)                                          │
│  Vite dev server · TanStack Query · shadcn/ui                        │
└──────────────┬───────────────────────────────────────────────────────┘
               │ /api/* (Vite proxy in dev)
               ▼
┌──────────────────────────────────────────────────────────────────────┐
│  mindsage binary (port 3003)                                         │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │  Axum HTTP Router (mindsage-server)                            │  │
│  │  CORS · tower middleware · 90+ API endpoints                   │  │
│  └──┬──────┬──────┬──────┬──────┬──────┬──────┬──────┬───────────┘  │
│     │      │      │      │      │      │      │      │              │
│  stats  vector  files  chat  browser localsend connectors privacy   │
│         store                                                        │
│     │      │      │      │      │      │      │      │              │
│  ┌──┴──────┴──────┴──────┴──────┴──────┴──────┴──────┴───────────┐  │
│  │  AppState (shared across all handlers)                         │  │
│  │  ┌─────────┐ ┌──────────┐ ┌────────┐ ┌──────────────────────┐ │  │
│  │  │SqliteStore│ │Embedder │ │LLMConfig│ │BrowserManager       │ │  │
│  │  │(FTS5+WAL)│ │(ONNX/   │ │(RwLock) │ │LocalSendServer      │ │  │
│  │  │         │ │ Noop)   │ │        │ │ConnectorManager     │ │  │
│  │  │         │ │         │ │        │ │PiiDetector          │ │  │
│  │  │         │ │         │ │        │ │ConsentManager       │ │  │
│  │  │         │ │         │ │        │ │Orchestrator         │ │  │
│  │  └─────────┘ └──────────┘ └────────┘ └──────────────────────┘ │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │  Background Tasks (tokio)                                      │  │
│  │  • Indexing worker (mpsc channel)                              │  │
│  │  • Embedding catch-up on startup                               │  │
│  │  • Extraction enrichment on startup                            │  │
│  └────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
               │
               ▼
┌──────────────────────────────────────────────────────────────────────┐
│  data/                                                               │
│  ├── vectordb/mindsage.db   SQLite (WAL mode, FTS5)                 │
│  ├── models/                ONNX model + tokenizer                   │
│  ├── uploads/               User-uploaded files                      │
│  ├── imports/               Queued for indexing                       │
│  ├── exports/               Connector export data                    │
│  ├── browser-connector/     Captured conversations                   │
│  ├── llm-config.json        LLM provider settings                   │
│  ├── connectors.json        Connector registry                       │
│  └── .indexed-files.json    Index state tracker                      │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Request Flow

### Search (most common path)

```
POST /api/vector-store/search  { query: "machine learning" }
        │
        ▼
  vector_store.rs::search()
        │
        ├─ Is embedder available?
        │    YES ──► embed query (mindsage-infer)
        │            ──► hybrid_search: BM25 + vector RRF k=60 (mindsage-store)
        │    NO  ──► bm25_search only (mindsage-store)
        │
        ▼
  Return JSON { results, total, query, search_type }
```

### Document Ingestion

```
POST /api/files/upload  (multipart)
        │
        ▼
  files.rs::upload()
  ──► Save to data/uploads/
  ──► Queue IndexingJob (mpsc sender)
        │
        ▼
  indexing.rs::process_indexing_job()
  ──► Detect file type (.txt, .md, .pdf, .json, code)
  ──► Extract text (mindsage-ingest::file)
  ──► Hierarchical chunk: sections (level=0) → paragraphs (level=1)
  ──► Store document + chunks (mindsage-store)
  ──► Embed level=1 chunks if embedder available (mindsage-infer)
  ──► Heuristic extraction: entities, topics, passages (mindsage-ingest::extract)
  ──► Write enriched_text back to chunks for FTS boosting
```

### RAG Chat

```
POST /api/chat/stream  { message: "What do I know about Rust?" }
        │
        ▼
  chat.rs::stream_chat()
  ──► Hybrid search for relevant chunks (top 5)
  ──► Build context from chunk text + metadata
  ──► Stream to external LLM (OpenAI / Anthropic / Groq)
  ──► SSE response: Token(string) | Done { tokens_used } | Error
```

### SDK Verbs (programmatic API)

```
Orchestrator (mindsage-runtime)
  │
  ├── ingest(text)    ──► chunk → embed → store → extract → topic
  ├── distill()       ──► batch embed pending + enrich unenriched
  ├── recall(query)   ──► tier-aware resolver → search → optional answer
  └── consolidate()   ──► prune orphans → deduplicate → evict old docs
```

---

## Crate Dependency Graph

```
mindsage-server (binary)
  ├── mindsage-core
  ├── mindsage-store ──── mindsage-core
  ├── mindsage-ingest ─── mindsage-core, mindsage-store
  ├── mindsage-infer ──── mindsage-core
  ├── mindsage-resolve ── mindsage-core, mindsage-store
  ├── mindsage-consolidate ── mindsage-core, mindsage-store
  ├── mindsage-runtime ── mindsage-core, mindsage-store, mindsage-ingest,
  │                       mindsage-infer, mindsage-consolidate, mindsage-resolve
  ├── mindsage-protocol ─ mindsage-core
  ├── mindsage-browser ── mindsage-core
  ├── mindsage-localsend ─ mindsage-core
  ├── mindsage-chat ───── mindsage-core
  └── mindsage-connectors ─ mindsage-core
```

`mindsage-core` is the leaf dependency — every crate depends on it. `mindsage-runtime` is the heaviest internal consumer, pulling in 6 sibling crates to orchestrate the SDK verbs.

---

## Directory Structure

```
mindsage-rs/
├── Cargo.toml                    # Workspace root, shared dependencies
├── Cargo.lock                    # Pinned dependency versions
├── .cargo/
│   └── config.toml               # Cross-compilation + release profiles
├── .gitignore                    # Excludes target/, models/, *.db
├── crates/
│   ├── mindsage-core/            # Foundation layer
│   ├── mindsage-store/           # Storage layer
│   ├── mindsage-ingest/          # Ingestion pipeline
│   ├── mindsage-infer/           # ML inference
│   ├── mindsage-resolve/         # Query resolution
│   ├── mindsage-consolidate/     # Memory maintenance
│   ├── mindsage-runtime/         # SDK orchestrator
│   ├── mindsage-protocol/        # Privacy & consent
│   ├── mindsage-server/          # HTTP server (binary)
│   ├── mindsage-browser/         # Browser automation
│   ├── mindsage-localsend/       # File transfer protocol
│   ├── mindsage-chat/            # LLM chat service
│   └── mindsage-connectors/      # Data source connectors
├── deploy/
│   ├── mindsage.service          # systemd unit file
│   ├── download-models.sh        # ONNX model downloader
│   └── README.md                 # Deployment guide
└── docs/
    ├── ARCHITECTURE.md           # This file
    └── PROGRESS.md               # Phase completion tracker
```

---

## Crate Details

### mindsage-core

Foundation types shared by every other crate.

```
crates/mindsage-core/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports all public types
    ├── capabilities.rs     # DeviceCapabilities, CapabilityTier
    ├── config.rs           # MindSageConfig, DataPaths
    └── error.rs            # Error enum, Result<T> alias
```

**Key types:**
- `CapabilityTier` — Base / Enhanced / Advanced / Full. Determined at startup by detecting RAM, CPU cores, GPU presence, and Jetson hardware. Controls search strategy, consolidation thresholds, and resource budgets throughout the system.
- `MindSageConfig` — Server configuration built from environment variables (`MINDSAGE_DATA_DIR`, `PORT`). Contains `DataPaths` which resolves all subdirectory paths (vectordb, uploads, imports, exports, browser-connector).
- `DeviceCapabilities` — Hardware inventory: total RAM, CPU count, GPU available, Jetson detection, capability tier.

---

### mindsage-store

SQLite storage with FTS5 full-text search, int8 vector embeddings, and a knowledge graph.

```
crates/mindsage-store/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── sqlite.rs           # SqliteStore — the main storage engine
    ├── types.rs            # Document, Chunk, SearchHit, StoreStats
    ├── schema.rs           # SQL DDL: tables, FTS5, triggers
    ├── embedding.rs        # int8 quantize/dequantize for vector storage
    └── graph.rs            # GraphBackend (petgraph, stub)
```

**SqliteStore** opens (or creates) a single `mindsage.db` file in WAL mode with these tables:

| Table | Purpose |
|-------|---------|
| `documents` | Full document text + metadata JSON + content_hash |
| `chunks` | Hierarchical chunks: level=0 (section), level=1 (paragraph) |
| `chunk_embeddings` | int8-quantized 384-dim vectors with scale/offset |
| `chunks_fts` | FTS5 virtual table over chunk text + enriched_text |

**Search methods:**
- `bm25_search(query, limit)` — FTS5 MATCH with bm25() scoring
- `vector_search(query_embedding, limit)` — int8 dot product against in-memory matrix
- `hybrid_search(query, query_embedding, limit)` — BM25 + vector with Reciprocal Rank Fusion (k=60)

The embedding matrix is loaded lazily into an `ndarray::Array2<u8>` on first vector search call. New embeddings are appended both to the matrix and to the database.

**12 tests** covering CRUD, search, deduplication, stats.

---

### mindsage-ingest

Text processing pipeline: file parsing, hierarchical chunking, and heuristic extraction.

```
crates/mindsage-ingest/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── chunking.rs         # RecursiveChunker (512 chars, 100 overlap)
    ├── ingest.rs           # Ingester — document → sections → paragraphs
    ├── file.rs             # File type detection + text extraction
    ├── extract.rs          # Heuristic extraction coordinator
    └── extract/
        ├── entities.rs     # Named entity extraction (regex-based)
        ├── topics.rs       # Topic extraction (TF-IDF style scoring)
        ├── passages.rs     # Key passage extraction (sentence scoring)
        ├── filters.rs      # Stop words, text normalization
        └── stemmer.rs      # Porter stemmer for term normalization
```

**Hierarchical chunking** splits a document into two levels:
1. **Sections** (level=0) — split on `\n\n\n+` or heading markers. These are parent containers, not directly searchable.
2. **Paragraphs** (level=1) — split within sections using RecursiveChunker (512 chars, 100 char overlap). These get embedded and are the search targets.

**Heuristic extraction** (no LLM needed) produces:
- **Entities**: email addresses, URLs, capitalized noun phrases, quoted terms
- **Topics**: scored by term frequency, filtered by stop words, stemmed for grouping
- **Key passages**: sentences scored by entity density, topic word overlap, and position

The extracted data is written to `enriched_text` on each chunk, which FTS5 indexes for boosted full-text search.

**Supported file types:** `.txt`, `.md`, `.pdf`, `.json` (ChatGPT export format), `.py`, `.js`, `.ts`, `.rs`, `.go`, `.java`, `.c`, `.cpp`, `.rb`

**15 tests** covering chunking, extraction, ingestion.

---

### mindsage-infer

Embedding engine with ONNX Runtime (feature-gated) and an LRU query cache.

```
crates/mindsage-infer/
├── Cargo.toml
└── src/
    ├── lib.rs              # create_embedder() factory, re-exports
    ├── embedder.rs         # EmbedderBackend trait, NoopEmbedder
    ├── onnx_embedder.rs    # OnnxEmbedder (feature = "onnx")
    └── cache.rs            # QueryCache — LRU with 1hr TTL
```

**EmbedderBackend trait:**
```rust
pub trait EmbedderBackend: Send + Sync {
    fn embed(&self, text: &str) -> Option<Array1<f32>>;
    fn embed_batch(&self, texts: &[&str]) -> Vec<Option<Array1<f32>>>;
    fn dimension(&self) -> usize;
    fn is_available(&self) -> bool;
}
```

**Two implementations:**
- `OnnxEmbedder` — Loads `all-MiniLM-L6-v2` (384-dim) via `ort` crate. Tokenizes with HuggingFace `tokenizers`. Mean-pools the last hidden state. Wrapped in `Mutex` because `ort::Session::run()` requires `&mut self`. Only compiled when `--features onnx` is set.
- `NoopEmbedder` — Returns `None` for all embed calls. `is_available()` returns `false`. Used when ONNX model files aren't present, gracefully degrading to BM25-only search.

**`create_embedder(model_dir)`** — Factory function that tries to load the ONNX model from `data/models/`. If `model.onnx` and `tokenizer.json` exist and the `onnx` feature is compiled in, returns `OnnxEmbedder`. Otherwise returns `NoopEmbedder`.

**QueryCache** — LRU cache (capacity 1000, 1hr TTL) mapping query strings to embedding vectors. Prevents re-embedding repeated search queries.

**3 tests** covering cache behavior.

---

### mindsage-resolve

Tier-aware query resolution — selects and executes search strategies.

```
crates/mindsage-resolve/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── hybrid.rs           # HybridResolver
    └── types.rs            # ResolveQuery, ResolveResult, ResolvedItem
```

**HybridResolver** wraps the store's search methods and selects strategy based on capability tier:
- **Base**: BM25 keyword search only
- **Enhanced+**: Hybrid BM25 + vector RRF when embedder is available

**ResolverKind** enum: Keyword, Entity, Vector, Hybrid, Timeline, Answer. The resolver tags each result with which strategy produced it.

**5 tests** covering resolution and tier behavior.

---

### mindsage-consolidate

Memory maintenance pipeline — keeps the database healthy over time.

```
crates/mindsage-consolidate/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── pipeline.rs         # ConsolidationPipeline
    └── types.rs            # ConsolidationReport, ConsolidationThresholds
```

**Pipeline stages:**
1. **PruneOrphans** — Remove chunks that reference deleted documents
2. **Deduplicate** — Remove documents with identical content_hash
3. **Evict** — Delete oldest documents when count exceeds tier threshold

**ConsolidationThresholds** adapt to hardware tier:
| Tier | Max Documents | Max Chunks |
|------|--------------|------------|
| Base | 1,000 | 10,000 |
| Enhanced | 5,000 | 50,000 |
| Advanced | 20,000 | 200,000 |
| Full | 100,000 | 1,000,000 |

**5 tests** covering each pipeline stage.

---

### mindsage-runtime

Orchestrator that ties everything together through the four SDK verbs.

```
crates/mindsage-runtime/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── orchestrator.rs     # Orchestrator + SDK verbs
    └── types.rs            # ResourceBudget
```

**SDK verbs:**

| Verb | What it does |
|------|-------------|
| `ingest(text, metadata)` | Chunk → embed → store → extract → update topics |
| `distill()` | Batch-embed unembedded chunks + enrich unenriched chunks. Returns `(enriched_count, embedded_count)` |
| `recall(query)` | Tier-aware resolver → hybrid search → return ranked results |
| `consolidate()` | Run the full consolidation pipeline (prune → dedup → evict) |

**ResourceBudget** sets memory limits per tier (Base: 512MB, Enhanced: 1GB, Advanced: 2GB, Full: 4GB).

**8 tests** covering all four verbs and edge cases.

---

### mindsage-protocol

Privacy layer — PII detection, token-based anonymization, and consent sessions.

```
crates/mindsage-protocol/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── pii.rs              # PiiDetector, PiiType, AnonymizationResult
    └── consent.rs          # ConsentSession, ConsentManager
```

**PII detection** uses 6 compiled regex patterns:

| PiiType | Pattern |
|---------|---------|
| Email | Standard email regex |
| Phone | US phone formats (with/without country code) |
| SSN | XXX-XX-XXXX |
| CreditCard | 13-19 digit sequences with optional separators |
| IpAddress | IPv4 dotted quad |
| URL | http(s):// URLs |

**Anonymization** replaces each PII match with `<PII:TYPE:UUID>` tokens. Tokens are stored in a session map for later de-anonymization. Sessions have a 1hr sliding TTL and LRU eviction (max 100 sessions).

**ConsentManager** tracks active consent sessions with data category filtering (personal, financial, health, location, communication). Presets: minimal, standard, full.

**9 tests** covering detection, anonymization, de-anonymization, and consent.

---

### mindsage-server

The HTTP server binary — Axum router, route handlers, background workers, and CLI.

```
crates/mindsage-server/
├── Cargo.toml
├── src/
│   ├── main.rs             # Entry point, CLI parsing, server startup
│   ├── state.rs            # AppState (shared state for all handlers)
│   ├── indexing.rs          # Background indexing worker + embedding catch-up
│   ├── migrate.rs           # validate() and migrate() for Python→Rust migration
│   └── routes/
│       ├── mod.rs           # Router builder — assembles all route groups
│       ├── stats.rs         # GET /api/stats, GET /api/server-info
│       ├── vector_store.rs  # Document CRUD, search, topics, graph
│       ├── files.rs         # Upload, list, delete, import
│       ├── indexing.rs       # Queue status, job list, cancel
│       ├── chat.rs          # RAG chat, streaming, LLM config
│       ├── browser.rs       # 30 browser connector endpoints
│       ├── localsend.rs     # 11 LocalSend endpoints
│       ├── connectors.rs   # 11 data connector endpoints
│       └── privacy.rs      # 10 PII/consent endpoints
└── tests/
    └── api_parity.rs       # 16 tests validating JSON shapes vs frontend
```

**CLI modes:**
```
mindsage                       Start the HTTP server (default)
mindsage validate [dir]        Validate a data directory's SQLite schema
mindsage migrate <src> [dst]   Copy data from Python installation
mindsage help                  Print usage
```

**Server startup sequence:**
1. Parse CLI args → dispatch to validate/migrate/help or continue to server
2. Resolve data directory from `MINDSAGE_DATA_DIR` env var
3. Open `SqliteStore` (creates tables if new)
4. Initialize embedder via `create_embedder()` (ONNX or Noop)
5. Load LLM config from `data/llm-config.json`
6. Build `AppState` with all managers
7. Spawn background indexing worker (processes queued files)
8. Run embedding catch-up (embed any chunks from prior sessions)
9. Run extraction catch-up (enrich any unenriched chunks)
10. Build Axum router with CORS and all route groups
11. Bind to `0.0.0.0:{PORT}` and serve

**API surface:** 90+ endpoints across 9 route modules. Every endpoint returns JSON matching the shapes expected by the React frontend's `api.ts` client.

**4 migration tests** + **16 API parity integration tests** = 20 tests.

---

### mindsage-browser

Chrome automation for capturing AI conversations from ChatGPT, Claude, and Gemini.

```
crates/mindsage-browser/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── manager.rs          # BrowserManager — Chrome lifecycle + CDP
    ├── config.rs           # BrowserConnectorConfig, site auth settings
    └── types.rs            # BrowserStatus, CapturedConversation, CaptureStats
```

**BrowserManager** handles:
- Chrome binary discovery (platform-specific paths)
- Launching Chrome with remote debugging via `tokio::process::Command`
- CDP WebSocket connection for cookie injection (`Network.setCookie`)
- Receiving captured conversations from the companion Chrome extension
- Auto-sync on a configurable interval (1-24 hours)
- Conversation deduplication and persistence to `data/browser-connector/`

**Supported sites:** ChatGPT, Claude, Gemini. The companion extension (unchanged JS, same Manifest V3) relays session cookies via `POST /api/browser-connector/import-cookies`.

---

### mindsage-localsend

LocalSend v2 protocol implementation for receiving files from phones and other devices on the local network.

```
crates/mindsage-localsend/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── server.rs           # LocalSendServer — sessions, discovery, file handling
    └── types.rs            # DeviceInfo, TransferSession, LocalSendStatus
```

**LocalSendServer** implements:
- UDP multicast discovery (224.0.0.167:53317) for device announcement
- Session-based file transfer (create session → receive files → complete)
- Device fingerprinting and discovery tracking
- Session TTL (1 hour) with automatic cleanup
- Files saved to `data/uploads/` and auto-queued for indexing

**Port 53317** is the standard LocalSend port. The server announces itself as "MindSage" on the local network.

**9 tests** covering sessions, file handling, discovery.

---

### mindsage-chat

RAG chat with external LLM providers — no local inference needed.

```
crates/mindsage-chat/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── config.rs           # LLMConfig — provider settings, persistence
    ├── types.rs            # ChatMessage, LLMProvider, StreamChunk
    └── providers.rs        # OpenAI/Groq (compatible) + Anthropic streaming
```

**Supported providers:**

| Provider | API Style | Default Model |
|----------|-----------|---------------|
| OpenAI | OpenAI-compatible | gpt-4o-mini |
| Groq | OpenAI-compatible | llama-3.3-70b-versatile |
| Anthropic | Messages API | claude-3-haiku-20240307 |

**LLMConfig** persists to `data/llm-config.json` and loads API keys from environment variables (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GROQ_API_KEY`).

**Streaming** uses SSE (Server-Sent Events). The `StreamChunk` enum carries `Token(String)`, `Done { tokens_used }`, or `Error(String)`.

---

### mindsage-connectors

Data source connectors for importing external data into the knowledge base.

```
crates/mindsage-connectors/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── manager.rs          # ConnectorManager — CRUD, persistence, sync
    ├── types.rs            # ConnectorConfig, ConnectorType, ConnectorStatus
    ├── chatgpt.rs          # ChatGPT ZIP export import
    └── facebook.rs         # Facebook ZIP export import + media extraction
```

**ConnectorManager** provides:
- CRUD for connector configurations (persisted to `data/connectors.json`)
- Sync orchestration with per-connector run status tracking
- Auto-indexing after successful import

**Connector types:**

| Type | Format | Notes |
|------|--------|-------|
| ChatGPT | ZIP (conversations.json) | Extracts conversation threads |
| Facebook | ZIP (messages/, posts/) | Handles Unicode escaping, media files |
| Notion | API | HTTP calls via reqwest (planned) |
| Readwise | API | Planned |
| Todoist | API | Planned |
| GitHub | API | Planned |

**13 tests** covering CRUD, import parsing, status tracking.

---

## Data Flow Summary

```
                    ┌─────────────┐
                    │  User Input  │
                    └──────┬──────┘
                           │
            ┌──────────────┼──────────────┐
            ▼              ▼              ▼
     File Upload    Browser Capture   LocalSend
     (multipart)   (extension POST)  (UDP + HTTP)
            │              │              │
            └──────────────┼──────────────┘
                           │
                           ▼
                  ┌─────────────────┐
                  │  Indexing Queue  │  (tokio mpsc)
                  └────────┬────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │  mindsage-ingest       │
              │  1. Parse file         │
              │  2. Section chunking   │
              │  3. Paragraph chunking │
              └────────────┬───────────┘
                           │
                    ┌──────┴──────┐
                    ▼             ▼
            ┌────────────┐ ┌────────────┐
            │ Store docs │ │ Embed L1   │
            │ + chunks   │ │ chunks     │
            │ (SQLite)   │ │ (ONNX)     │
            └────────────┘ └────────────┘
                    │             │
                    ▼             ▼
            ┌────────────┐ ┌────────────┐
            │ Extract    │ │ Store int8 │
            │ entities,  │ │ vectors    │
            │ topics,    │ │ (SQLite)   │
            │ passages   │ └────────────┘
            └─────┬──────┘
                  │
                  ▼
            ┌────────────┐
            │ Enrich FTS │  (enriched_text)
            │ for search │
            │ boosting   │
            └────────────┘
```

---

## Search Architecture

```
                  Query: "machine learning transformers"
                           │
                    ┌──────┴──────┐
                    ▼             ▼
            ┌────────────┐ ┌────────────┐
            │ BM25 Search│ │ Vector     │
            │ (FTS5      │ │ Search     │
            │  MATCH +   │ │ (int8 dot  │
            │  bm25()    │ │  product)  │
            │  scoring)  │ │            │
            └─────┬──────┘ └─────┬──────┘
                  │              │
                  ▼              ▼
            ┌────────────────────────┐
            │  Reciprocal Rank       │
            │  Fusion (k=60)         │
            │                        │
            │  score = Σ 1/(k + rank)│
            └───────────┬────────────┘
                        │
                        ▼
            ┌────────────────────────┐
            │  Post-processing       │
            │  • Entity boost (+0.15)│
            │  • Dedup (best/doc)    │
            │  • Passage extraction  │
            └───────────┬────────────┘
                        │
                        ▼
                  JSON response
```

When the ONNX embedder is not available, the vector branch is skipped and results come from BM25 alone. This is transparent to the frontend.

---

## Deployment

### Ports

| Port | Protocol | Service |
|------|----------|---------|
| 3003 | HTTP | Main API (Axum) |
| 53317 | UDP + HTTP | LocalSend file transfer |
| 6080 | WebSocket | VNC proxy (browser connector, optional) |

### Build Profiles

| Profile | opt-level | LTO | Use case |
|---------|-----------|-----|----------|
| `release` | 3 | thin | Desktop / CI |
| `release-jetson` | s (size) | full | Jetson Orin Nano |

### Cross-Compilation

```bash
# Using cross (Docker-based, easiest)
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu --features mindsage-infer/onnx

# Binary at: target/aarch64-unknown-linux-gnu/release/mindsage
```

### systemd Service

The `deploy/mindsage.service` runs the binary as a dedicated `mindsage` user with:
- `ProtectSystem=strict` — read-only filesystem except data dir
- `NoNewPrivileges=true` — no privilege escalation
- `MemoryMax=4G` — stays within Jetson's 8GB shared memory
- `Restart=on-failure` with 5s delay

---

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| **No local LLM** | Heuristic extraction covers entity/topic/passage needs. Saves 2GB GPU memory on Jetson. External LLMs via API for chat. |
| **Feature-gated ONNX** | `--features onnx` adds ~40MB to binary. Without it, server still works with BM25-only search. Useful for quick dev builds. |
| **Same SQLite schema** | Rust reads/writes the same `mindsage.db` as Python. Zero-downtime migration — just swap the binary. |
| **Same API surface** | Frontend `api.ts` doesn't change. 16 API parity tests validate response shapes. |
| **int8 quantization** | 384-dim float32 → uint8 reduces embedding storage 4x. Scale+offset per vector preserves search quality. |
| **Hierarchical chunks** | Section (level=0) provides parent context for passage extraction. Paragraph (level=1) is the search unit. |
| **RRF over learned fusion** | Reciprocal Rank Fusion (k=60) needs no training, works well for combining BM25 + vector rankings. |
| **Single binary** | No runtime dependencies (Node.js, Python, pip). Just the binary + ONNX model files + SQLite database. |

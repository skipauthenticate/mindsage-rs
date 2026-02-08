# MindSage Deployment Guide

## Quick Start (Same Architecture)

```bash
# Build release binary
cargo build --release

# Download embedding model
./deploy/download-models.sh ./data/models

# Start server
MINDSAGE_DATA_DIR=./data ./target/release/mindsage
```

## Cross-Compilation for Jetson (aarch64)

### Option A: Using `cross` (Recommended)

```bash
# Install cross
cargo install cross

# Build for Jetson
cross build --release --target aarch64-unknown-linux-gnu
```

### Option B: Native Cross-Compilation (macOS)

```bash
# Install cross-compiler toolchain
brew install aarch64-unknown-linux-gnu

# Add Rust target
rustup target add aarch64-unknown-linux-gnu

# Build
cargo build --release --target aarch64-unknown-linux-gnu
```

The binary will be at `target/aarch64-unknown-linux-gnu/release/mindsage`.

## Jetson Deployment

### 1. Copy files to Jetson

```bash
# Copy binary
scp target/aarch64-unknown-linux-gnu/release/mindsage jetson:/opt/mindsage/bin/

# Copy deployment files
scp deploy/mindsage.service jetson:/etc/systemd/system/
scp deploy/download-models.sh jetson:/opt/mindsage/
```

### 2. Set up on Jetson

```bash
# Create user and directories
sudo useradd -r -s /bin/false mindsage
sudo mkdir -p /opt/mindsage/{bin,data/models,data/uploads,data/imports,data/vectordb}
sudo chown -R mindsage:mindsage /opt/mindsage

# Download models
cd /opt/mindsage && sudo -u mindsage ./download-models.sh ./data/models
```

### 3. Migrate existing data (optional)

If migrating from the Python/Node.js installation:

```bash
# Validate existing data
./bin/mindsage validate /path/to/old/data

# Run migration
./bin/mindsage migrate /path/to/old/data /opt/mindsage/data
```

### 4. Start the service

```bash
sudo systemctl daemon-reload
sudo systemctl enable mindsage
sudo systemctl start mindsage

# Check status
sudo systemctl status mindsage
journalctl -u mindsage -f
```

## CLI Commands

```
mindsage                     Start the server (default)
mindsage validate [dir]      Validate an existing database
mindsage migrate <src> [dst] Migrate data from Python installation
mindsage help                Show help
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MINDSAGE_DATA_DIR` | `./data` | Data directory path |
| `MINDSAGE_PORT` | `3003` | HTTP server port |
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |

## Directory Layout

```
/opt/mindsage/
├── bin/mindsage              # Server binary
├── data/
│   ├── vectordb/mindsage.db  # SQLite database
│   ├── models/               # ONNX model files
│   │   ├── model.onnx
│   │   └── tokenizer.json
│   ├── uploads/              # User uploads
│   ├── imports/              # Import queue
│   ├── exports/              # Connector exports
│   ├── browser-connector/    # Capture data
│   ├── llm-config.json       # LLM provider config
│   └── .indexed-files.json   # Index state
└── deploy/
    ├── mindsage.service      # systemd unit
    └── download-models.sh    # Model downloader
```

## Building with ONNX Support

To enable the ONNX embedding engine (recommended for production):

```bash
cargo build --release --features mindsage-infer/onnx
```

Without the `onnx` feature, the server falls back to BM25-only search (no vector embeddings).

## Ports

| Port | Service |
|------|---------|
| 3003 | HTTP API |
| 53317 | LocalSend file transfer |
| 6080 | VNC (WebSocket proxy) |

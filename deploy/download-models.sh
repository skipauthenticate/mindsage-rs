#!/bin/bash
# Download ONNX model files for MindSage embedding engine.
#
# Downloads all-MiniLM-L6-v2 ONNX model and tokenizer from HuggingFace.
# Place this in the data/models/ directory.
#
# Usage: ./download-models.sh [target-dir]

set -euo pipefail

TARGET_DIR="${1:-./data/models}"
MODEL_NAME="sentence-transformers/all-MiniLM-L6-v2"
HF_BASE="https://huggingface.co/${MODEL_NAME}/resolve/main"

echo "=== MindSage Model Downloader ==="
echo "Target directory: ${TARGET_DIR}"
echo "Model: ${MODEL_NAME}"
echo ""

mkdir -p "${TARGET_DIR}"

# Download ONNX model
echo "Downloading model.onnx..."
if command -v curl &> /dev/null; then
    curl -L -o "${TARGET_DIR}/model.onnx" "${HF_BASE}/onnx/model.onnx"
elif command -v wget &> /dev/null; then
    wget -O "${TARGET_DIR}/model.onnx" "${HF_BASE}/onnx/model.onnx"
else
    echo "Error: curl or wget required"
    exit 1
fi

# Download tokenizer
echo "Downloading tokenizer.json..."
if command -v curl &> /dev/null; then
    curl -L -o "${TARGET_DIR}/tokenizer.json" "${HF_BASE}/tokenizer.json"
else
    wget -O "${TARGET_DIR}/tokenizer.json" "${HF_BASE}/tokenizer.json"
fi

# Download tokenizer config
echo "Downloading tokenizer_config.json..."
if command -v curl &> /dev/null; then
    curl -L -o "${TARGET_DIR}/tokenizer_config.json" "${HF_BASE}/tokenizer_config.json"
else
    wget -O "${TARGET_DIR}/tokenizer_config.json" "${HF_BASE}/tokenizer_config.json"
fi

# Verify files
echo ""
echo "Downloaded files:"
ls -lh "${TARGET_DIR}/"

echo ""
echo "Model download complete. Start MindSage with:"
echo "  MINDSAGE_DATA_DIR=./data ./mindsage"

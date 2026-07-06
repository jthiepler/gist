#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

rm -rf build dist/gist-sidecar

echo "=== Building gist-sidecar with PyInstaller ==="

uv run pyinstaller \
    --onedir \
    --name gist-sidecar \
    --noconfirm \
    --clean \
    --log-level WARN \
    --collect-all mlx_lm \
    --collect-submodules mlx_lm.models \
    --collect-all transformers \
    --collect-all huggingface_hub \
    --collect-all faster_whisper \
    --collect-all ctranslate2 \
    --collect-all tokenizers \
    --collect-all numpy \
    --collect-all mlx \
    --collect-all mlx_audio \
    --collect-all scipy \
    --collect-all miniaudio \
    --collect-all sentencepiece \
    --runtime-hook gist/_runtime_hook.py \
    --exclude torch \
    --exclude torchvision \
    --exclude torchaudio \
    --exclude onnx \
    --exclude onnxruntime \
    --exclude tensorflow \
    --exclude keras \
    --exclude tf2onnx \
    --exclude flax \
    --exclude jax \
    --exclude librosa \
    run_sidecar.py

echo "=== Build complete: dist/gist-sidecar/ ==="
echo "Size: $(du -sh dist/gist-sidecar/ | cut -f1)"

# Copy to Tauri resources
echo "=== Copying to src-tauri/resources/ ==="
mkdir -p src-tauri/resources
rm -rf src-tauri/resources/gist-sidecar
cp -R dist/gist-sidecar src-tauri/resources/gist-sidecar
echo "Done"

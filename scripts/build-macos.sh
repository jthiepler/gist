#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

DIARIZATION_SOURCE="$PROJECT_DIR/speaker-diarization-community-1"
DIARIZATION_RESOURCE="$PROJECT_DIR/src-tauri/resources/pyannote/speaker-diarization-community-1"
PARAKEET_SOURCE="$PROJECT_DIR/parakeet-tdt-0.6b-v3-mlx-4bit"
PARAKEET_RESOURCE="$PROJECT_DIR/src-tauri/resources/parakeet/parakeet-tdt-0.6b-v3-mlx-4bit"
if [ ! -f "$DIARIZATION_SOURCE/config.yaml" ]; then
    echo "Missing local PyAnnote model checkout: $DIARIZATION_SOURCE" >&2
    echo "Clone or copy speaker-diarization-community-1 into the project root before building." >&2
    exit 1
fi
if [ ! -f "$PARAKEET_SOURCE/config.json" ] || [ ! -f "$PARAKEET_SOURCE/model.safetensors" ]; then
    echo "Missing local quantized Parakeet model checkout: $PARAKEET_SOURCE" >&2
    echo "Clone or copy parakeet-tdt-0.6b-v3-mlx-4bit into the project root before building." >&2
    exit 1
fi

TORCHCODEC_DIR="$(uv run python -c 'import pathlib, torchcodec; print(pathlib.Path(torchcodec.__file__).parent)')"
PYTHON_LIB="$(uv run python -c 'import pathlib, sysconfig; print(pathlib.Path(sysconfig.get_config_var("LIBDIR")) / "libpython3.13.dylib")')"

rm -rf build dist/gist-sidecar

echo "=== Building gist-sidecar with PyInstaller ==="

uv run --group dev pyinstaller \
    --onedir \
    --name gist-sidecar \
    --noconfirm \
    --clean \
    --log-level WARN \
    --specpath build \
    --collect-all mlx_lm \
    --collect-submodules mlx_lm.models \
    --collect-all transformers \
    --collect-all huggingface_hub \
    --collect-all tokenizers \
    --collect-all numpy \
    --collect-all mlx \
    --collect-all mlx_audio \
    --collect-all pyannote.audio \
    --hidden-import torch \
    --hidden-import torchaudio \
    --hidden-import torchcodec \
    --collect-submodules torchcodec \
    --add-binary "$TORCHCODEC_DIR/libtorchcodec_core8.dylib:torchcodec" \
    --add-binary "$TORCHCODEC_DIR/libtorchcodec_custom_ops8.dylib:torchcodec" \
    --add-binary "$TORCHCODEC_DIR/libtorchcodec_pybind_ops8.so:torchcodec" \
    --add-binary "$TORCHCODEC_DIR/.dylibs/libc++.1.0.dylib:torchcodec/.dylibs" \
    --collect-all scipy \
    --collect-all miniaudio \
    --collect-all sentencepiece \
    --add-data "gist/formats/defaults.json:gist/formats" \
    --runtime-hook gist/_runtime_hook.py \
    --exclude torchvision \
    --exclude onnx \
    --exclude onnxruntime \
    --exclude tensorflow \
    --exclude keras \
    --exclude tf2onnx \
    --exclude flax \
    --exclude jax \
    --exclude librosa \
    run_sidecar.py

# TorchCodec ships a private libpython for its native extension. PyInstaller
# may let that file replace the sidecar's main runtime library by basename.
# Restore the PyInstaller runtime at the top level while keeping TorchCodec's
# private copy in torchcodec/.dylibs.
BUNDLED_PYTHON_LIB="dist/gist-sidecar/_internal/$(basename "$PYTHON_LIB")"
if [ -L "$BUNDLED_PYTHON_LIB" ]; then
    rm -f "$BUNDLED_PYTHON_LIB"
    cp "$PYTHON_LIB" "$BUNDLED_PYTHON_LIB"
fi

echo "=== Build complete: dist/gist-sidecar/ ==="
echo "Size: $(du -sh dist/gist-sidecar/ | cut -f1)"

# Copy to Tauri resources
echo "=== Copying to src-tauri/resources/ ==="
mkdir -p src-tauri/resources
rm -rf src-tauri/resources/gist-sidecar
cp -R dist/gist-sidecar src-tauri/resources/gist-sidecar
mkdir -p "$(dirname "$PARAKEET_RESOURCE")"
rm -rf "$PARAKEET_RESOURCE"
mkdir -p "$PARAKEET_RESOURCE"
rsync -a --exclude='.git' --exclude='.DS_Store' "$PARAKEET_SOURCE/" "$PARAKEET_RESOURCE/"
mkdir -p "$(dirname "$DIARIZATION_RESOURCE")"
rm -rf "$DIARIZATION_RESOURCE"
mkdir -p "$DIARIZATION_RESOURCE"
cp "$DIARIZATION_SOURCE/config.yaml" "$DIARIZATION_SOURCE/README.md" "$DIARIZATION_RESOURCE/"
cp -R \
    "$DIARIZATION_SOURCE/embedding" \
    "$DIARIZATION_SOURCE/plda" \
    "$DIARIZATION_SOURCE/segmentation" \
    "$DIARIZATION_RESOURCE/"
cp THIRD_PARTY_NOTICES.md src-tauri/resources/THIRD_PARTY_NOTICES.md
echo "Done"

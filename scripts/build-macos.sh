#!/usr/bin/env bash
set -euo pipefail

# The macOS `shasum` utility is Perl-based. A developer's shell may export a
# locale that is not installed (for example, en_FR.UTF-8), which causes one
# warning per hash invocation while fingerprinting model trees. Keep the
# packaging environment deterministic and avoid leaking those warnings into
# release logs; this does not alter the app's runtime locale.
export LC_ALL=C
export LANG=C

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
CACHE_DIR="$PROJECT_DIR/.cache/gist-build"

MODE="development"
FORCE=false

usage() {
    cat <<'EOF'
Usage: bash scripts/build-macos.sh [--mode development|release] [--force]

  development  Reuse the sidecar and model resources when their inputs match.
  release      Rebuild the sidecar from a clean PyInstaller work directory.
  --force      Rebuild the sidecar and resync model resources regardless of cache.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mode)
            MODE="${2:-}"
            shift 2
            ;;
        --force)
            FORCE=true
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "$MODE" != "development" && "$MODE" != "release" ]]; then
    echo "Build mode must be 'development' or 'release'." >&2
    exit 2
fi

cd "$PROJECT_DIR"
mkdir -p "$CACHE_DIR"

DIARIZATION_SOURCE="$PROJECT_DIR/speaker-diarization-community-1"
DIARIZATION_RESOURCE="$PROJECT_DIR/src-tauri/resources/pyannote/speaker-diarization-community-1"
PARAKEET_SOURCE="$PROJECT_DIR/parakeet-tdt-0.6b-v3-mlx-4bit"
PARAKEET_RESOURCE="$PROJECT_DIR/src-tauri/resources/parakeet/parakeet-tdt-0.6b-v3-mlx-4bit"
SIDECAR_RESOURCE="$PROJECT_DIR/src-tauri/resources/gist-sidecar"

if [[ ! -f "$DIARIZATION_SOURCE/config.yaml" ]]; then
    echo "Missing local pyannote model checkout: $DIARIZATION_SOURCE" >&2
    echo "Clone or copy speaker-diarization-community-1 into the project root before building." >&2
    exit 1
fi
if [[ ! -f "$PARAKEET_SOURCE/config.json" || ! -f "$PARAKEET_SOURCE/model.safetensors" ]]; then
    echo "Missing local quantized Parakeet model checkout: $PARAKEET_SOURCE" >&2
    echo "Clone or copy parakeet-tdt-0.6b-v3-mlx-4bit into the project root before building." >&2
    exit 1
fi

hash_file() {
    shasum -a 256 "$1" | awk '{print $1}'
}

hash_tree() {
    local root="$1"
    (
        cd "$root"
        find . -type f \
            ! -path './.git/*' \
            ! -name '.DS_Store' \
            -print |
            LC_ALL=C sort |
            while IFS= read -r file; do
                printf '%s  %s\n' "$(hash_file "$file")" "$file"
            done
    ) | shasum -a 256 | awk '{print $1}'
}

combined_hash() {
    shasum -a 256 | awk '{print $1}'
}

marker_matches() {
    local marker="$1"
    local expected="$2"
    [[ -f "$marker" ]] && [[ "$(<"$marker")" == "$expected" ]]
}

write_marker() {
    local marker="$1"
    local value="$2"
    local temporary="$marker.tmp"
    printf '%s\n' "$value" > "$temporary"
    mv "$temporary" "$marker"
}

PYTHON_BUILD_ID="$(uv run python -c 'import platform, sys; print(f"{sys.implementation.name}-{platform.machine()}-{sys.version}")')"
PYINSTALLER_VERSION="$(uv run --group dev pyinstaller --version)"
SIDECAR_HASH="$({
    printf '%s\n' "$PYTHON_BUILD_ID" "$PYINSTALLER_VERSION"
    printf '%s\n' "$(hash_file gist-sidecar.spec)"
    printf '%s\n' "$(hash_file pyproject.toml)"
    printf '%s\n' "$(hash_file uv.lock)"
    printf '%s\n' "$(hash_file run_sidecar.py)"
    printf '%s\n' "$(hash_tree gist)"
} | combined_hash)"
SIDECAR_MARKER="$CACHE_DIR/sidecar.sha256"

build_sidecar=false
if [[ "$MODE" == "release" || "$FORCE" == true ]]; then
    build_sidecar=true
elif ! marker_matches "$SIDECAR_MARKER" "$SIDECAR_HASH"; then
    build_sidecar=true
elif [[ ! -x "$SIDECAR_RESOURCE/gist-sidecar" || ! -d "$SIDECAR_RESOURCE/_internal" ]]; then
    build_sidecar=true
fi

if [[ "$build_sidecar" == true ]]; then
    PYTHON_LIB="$(uv run python -c 'import pathlib, sysconfig; print(pathlib.Path(sysconfig.get_config_var("LIBDIR")) / sysconfig.get_config_var("LDLIBRARY"))')"
    if [[ ! -f "$PYTHON_LIB" ]]; then
        echo "Could not locate the active Python runtime library: $PYTHON_LIB" >&2
        exit 1
    fi

    echo "=== Building gist-sidecar ($MODE mode) ==="
    rm -rf "$PROJECT_DIR/dist/gist-sidecar"
    pyinstaller_args=(
        --noconfirm
        --log-level WARN
        --distpath "$PROJECT_DIR/dist"
        --workpath "$PROJECT_DIR/build/pyinstaller"
    )
    if [[ "$MODE" == "release" ]]; then
        rm -rf "$PROJECT_DIR/build/pyinstaller"
        pyinstaller_args+=(--clean)
    fi
    uv run --group dev pyinstaller "${pyinstaller_args[@]}" gist-sidecar.spec

    # TorchCodec ships a private libpython for its native extension. PyInstaller
    # may let that file replace the sidecar's main runtime library by basename.
    # Restore the active PyInstaller runtime at the top level while retaining
    # TorchCodec's private copy in torchcodec/.dylibs.
    BUNDLED_PYTHON_LIB="$PROJECT_DIR/dist/gist-sidecar/_internal/$(basename "$PYTHON_LIB")"
    if [[ -L "$BUNDLED_PYTHON_LIB" ]]; then
        rm -f "$BUNDLED_PYTHON_LIB"
        cp "$PYTHON_LIB" "$BUNDLED_PYTHON_LIB"
    fi

    mkdir -p "$SIDECAR_RESOURCE"
    rsync -a --delete "$PROJECT_DIR/dist/gist-sidecar/" "$SIDECAR_RESOURCE/"
    write_marker "$SIDECAR_MARKER" "$SIDECAR_HASH"
    echo "Sidecar size: $(du -sh "$SIDECAR_RESOURCE" | cut -f1)"
else
    echo "=== Reusing cached gist-sidecar ($SIDECAR_HASH) ==="
fi

PARAKEET_HASH="$(hash_tree "$PARAKEET_SOURCE")"
PARAKEET_MARKER="$CACHE_DIR/parakeet.sha256"
if [[ "$FORCE" == true ]] ||
    ! marker_matches "$PARAKEET_MARKER" "$PARAKEET_HASH" ||
    [[ ! -f "$PARAKEET_RESOURCE/model.safetensors" ]]; then
    echo "=== Synchronizing Parakeet model ==="
    mkdir -p "$PARAKEET_RESOURCE"
    rsync -a --delete \
        --exclude='.git' \
        --exclude='.DS_Store' \
        "$PARAKEET_SOURCE/" "$PARAKEET_RESOURCE/"
    write_marker "$PARAKEET_MARKER" "$PARAKEET_HASH"
else
    echo "=== Parakeet model unchanged; keeping existing resource ==="
fi

DIARIZATION_HASH="$({
    printf '%s\n' "$(hash_file "$DIARIZATION_SOURCE/config.yaml")"
    printf '%s\n' "$(hash_file "$DIARIZATION_SOURCE/README.md")"
    printf '%s\n' "$(hash_tree "$DIARIZATION_SOURCE/embedding")"
    printf '%s\n' "$(hash_tree "$DIARIZATION_SOURCE/plda")"
    printf '%s\n' "$(hash_tree "$DIARIZATION_SOURCE/segmentation")"
} | combined_hash)"
DIARIZATION_MARKER="$CACHE_DIR/pyannote.sha256"
if [[ "$FORCE" == true ]] ||
    ! marker_matches "$DIARIZATION_MARKER" "$DIARIZATION_HASH" ||
    [[ ! -f "$DIARIZATION_RESOURCE/config.yaml" ]]; then
    echo "=== Synchronizing pyannote model ==="
    mkdir -p "$DIARIZATION_RESOURCE"
    rsync -a --delete --delete-excluded \
        --include='/config.yaml' \
        --include='/README.md' \
        --include='/embedding/***' \
        --include='/plda/***' \
        --include='/segmentation/***' \
        --exclude='*' \
        "$DIARIZATION_SOURCE/" "$DIARIZATION_RESOURCE/"
    write_marker "$DIARIZATION_MARKER" "$DIARIZATION_HASH"
else
    echo "=== pyannote model unchanged; keeping existing resource ==="
fi

mkdir -p "$PROJECT_DIR/src-tauri/resources"
if [[ ! -f "$PROJECT_DIR/src-tauri/resources/THIRD_PARTY_NOTICES.md" ]] ||
    ! cmp -s "$PROJECT_DIR/THIRD_PARTY_NOTICES.md" "$PROJECT_DIR/src-tauri/resources/THIRD_PARTY_NOTICES.md"; then
    cp "$PROJECT_DIR/THIRD_PARTY_NOTICES.md" "$PROJECT_DIR/src-tauri/resources/THIRD_PARTY_NOTICES.md"
fi

echo "=== macOS resources ready ($MODE mode) ==="

<!-- headroom:rtk-instructions -->
# RTK (Rust Token Killer) - Token-Optimized Commands

When running shell commands, **always prefix with `rtk`**. This reduces context
usage by 60-90% with zero behavior change. If rtk has no filter for a command,
it passes through unchanged — so it is always safe to use.

## Key Commands
```bash
# Git (59-80% savings)
rtk git status          rtk git diff            rtk git log

# Files & Search (60-75% savings)
rtk ls <path>           rtk read <file>         rtk grep <pattern>
rtk find <pattern>      rtk diff <file>

# Test (90-99% savings) — shows failures only
rtk pytest tests/       rtk cargo test          rtk test <cmd>

# Build & Lint (80-90% savings) — shows errors only
rtk tsc                 rtk lint                rtk cargo build
rtk prettier --check    rtk mypy                rtk ruff check

# Analysis (70-90% savings)
rtk err <cmd>           rtk log <file>          rtk json <file>
rtk summary <cmd>       rtk deps                rtk env

# GitHub (26-87% savings)
rtk gh pr view <n>      rtk gh run list         rtk gh issue list

# Infrastructure (85% savings)
rtk docker ps           rtk kubectl get         rtk docker logs <c>

# Package managers (70-90% savings)
rtk pip list            rtk pnpm install        rtk npm run <script>
```

## Rules
- In command chains, prefix each segment: `rtk git add . && rtk git commit -m "msg"`
- For debugging, use raw command without rtk prefix
- `rtk proxy <cmd>` runs command without filtering but tracks usage
<!-- /headroom:rtk-instructions -->

<!-- headroom:project-context -->

# Project: Gist

Local-first macOS desktop app for therapists. Transcribes session audio and
generates structured clinical notes using a locally bundled LLM.

## Python Sidecar (`gist/`)

CLI + JSON-RPC server for transcription and clinical note generation.

- `uv run gist <command>` to run
- `bash scripts/build-macos.sh` to build with PyInstaller (output: dist/gist-sidecar/)
- Entry point: `run_sidecar.py` (thin wrapper importing gist.__main__:cli)

### CLI Commands
- `transcribe AUDIO_FILE` — Transcribe audio to text
- `note` — Generate clinical note from transcript (stdin or file)
- `formats` — List note formats (soap, cbt, intake)
- `models list` — List available models
- `download [MODEL]` — Download models from HuggingFace
- `serve` — JSON-RPC server (stdin/stdout, newline-delimited JSON)

### JSON-RPC Protocol
- Request: `{"type": "transcribe|generate_note|download_model|ping|exit|list_models|list_formats", ...}`
- Progress: `{"type": "progress", "percent": N, "stage": "..."}`
- Result: `{"type": "result", ...}`
- Error: `{"type": "error", "message": "..."}`

### Default Models
- LLM: qwen-3.5-4b (MLX, ~2.5 GB)
- Transcription: parakeet-tdt-0.6b-v3 (~230 MB)

### Backends
- LLM: MLX (macOS) or OpenAI-compatible (debug)
- Transcription: faster-whisper (CPU) or parakeet-mlx (Apple Silicon)

## Tauri Shell (`src-tauri/` + `src/`)

- Rust backend manages Python sidecar lifecycle, JSON-RPC IPC
- SvelteKit frontend (SPA mode with adapter-static)
- Commands: `start_sidecar`, `stop_sidecar`, `rpc_call`, `is_running`
- Sidecar bundled in .app at Resources/resources/gist-sidecar/

## Build

```bash
# 1. Build Python sidecar
bash scripts/build-macos.sh

# 2. Build full Tauri .app
npm run tauri build

# Output:
#   src-tauri/target/release/bundle/macos/Gist.app  (~296 MB)
#   src-tauri/target/release/bundle/dmg/Gist_0.1.0_aarch64.dmg
```

## Key Architecture

- Fully stateless Python sidecar (no DB)
- MLX for macOS native LLM inference
- JSON-RPC over stdin/stdout with progress streaming
- Async Rust bridges Tauri frontend to Python sidecar
- Tauri owns all storage (SQLite in Rust, future)

<!-- /headroom:project-context -->

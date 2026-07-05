# Gist

Local-first macOS desktop app for therapists. Transcribes session audio and
generates structured clinical notes using a locally bundled LLM — fully offline,
private, no cloud dependency.

## Architecture

- **Tauri 2 + SvelteKit** shell (Rust backend, Svelte SPA frontend)
- **Python sidecar** (`gist/`) for transcription + LLM inference, bundled via PyInstaller
- **MLX** for native Apple Silicon LLM inference
- **faster-whisper** for transcription
- JSON-RPC over stdin/stdout between Rust and Python
- SQLite (owned by Tauri) for patients, sessions, and settings

## Build

```bash
# 1. Build Python sidecar
bash scripts/build-macos.sh

# 2. Build full Tauri .app
npm run tauri build

# Output:
#   src-tauri/target/release/bundle/macos/Gist.app
#   src-tauri/target/release/bundle/dmg/Gist_0.1.0_aarch64.dmg
```

## Python Sidecar

```bash
uv run gist <command>
```

Commands: `transcribe`, `note`, `formats`, `models`, `download`, `serve`.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).

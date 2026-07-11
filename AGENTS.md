# Repository Guidelines

## Project Structure & Module Organization

Gist is a Tauri 2 macOS desktop app with three layers:

- `src/` contains the SvelteKit SPA: routes in `src/routes/`, shared logic in `src/lib/`, and UI components in `src/lib/components/`.
- `src-tauri/` contains the Rust application, SQLite persistence, audio recording, sidecar lifecycle, and Tauri commands.
- `gist/` is the Python CLI/JSON-RPC sidecar. Its formats, LLM backends, and transcription backends live in `gist/formats/`, `gist/llm/`, and `gist/transcription/`.
- `scripts/` holds packaging scripts; `static/` and `icon/` hold assets. `test*_soap.md` files are sample note inputs.

## Build, Test, and Development Commands

Install dependencies with `npm install` and `uv sync`.

- `npm run dev` — start the Vite frontend development server.
- `npm run check` — run SvelteKit synchronization and `svelte-check`.
- `cargo check --manifest-path src-tauri/Cargo.toml` — type-check the Rust backend.
- `uv run gist formats` — smoke-test the sidecar CLI without downloading models.
- `bash scripts/build-macos.sh` — build the PyInstaller sidecar and copy it into Tauri resources; requires the local diarization model checkout.
- `npm run tauri build` — build the complete macOS application and installer.

There is no dedicated automated test suite yet. Run the checks above and manually verify affected UI flows in `npm run tauri dev`.

## Coding Style & Naming Conventions

Use two-space indentation in Svelte, TypeScript, and JSON; four spaces in Python. Use `PascalCase` for components and exported types, `camelCase` for TypeScript functions and variables, and `snake_case` for Python and Rust functions. Format Rust with `cargo fmt` and keep component-specific styles close to their components.

## Testing Guidelines

Add focused checks or fixtures when introducing behavior that can be tested without model inference. For sidecar changes, exercise the relevant CLI command or JSON-RPC request and include representative sample input where useful. Do not require model downloads for routine validation.

## Commit & Pull Request Guidelines

Use short, imperative commit subjects; the history uses descriptive subjects and prefixes such as `feat:`, `fix:`, and `refactor:`. Pull requests should explain the change, list validation commands, link issues, and include screenshots for UI changes. Call out macOS permissions, model/resource changes, and privacy implications.

## Security & Configuration

The app handles sensitive clinical data. Never commit recordings, transcripts, downloaded models, generated bundles, credentials, or patient data. Keep generated sidecar files under ignored build/resource paths and review `THIRD_PARTY_NOTICES.md` when changing bundled dependencies.

# Credits and acknowledgments

Gist is possible because of the open-source projects and model authors below.
Please consult each upstream project for the current license and attribution
requirements before redistributing a release bundle.

## Application and frontend

- [Tauri 2](https://v2.tauri.app/) and [tauri-apps/plugins](https://github.com/tauri-apps/plugins-workspace)
  provide the desktop shell, IPC, dialogs, and opener integration.
- [Rust](https://www.rust-lang.org/), [serde](https://serde.rs/),
  [serde_json](https://github.com/serde-rs/json), [Tokio](https://tokio.rs/),
  [rusqlite](https://github.com/rusqlite/rusqlite),
  [uuid](https://github.com/uuid-rs/uuid), [chrono](https://github.com/chronotope/chrono),
  [anyhow](https://github.com/dtolnay/anyhow), [log](https://github.com/rust-lang/log),
  and [libc](https://github.com/rust-lang/libc) support the native application,
  persistence, concurrency, and error handling.
- [cpal](https://github.com/RustAudio/cpal), [ringbuf](https://github.com/ageron/ringbuf),
  [hound](https://github.com/ruuda/hound), and
  [cidre](https://github.com/yury/cidre) support local macOS audio capture and
  processing. (The cpal and cidre revisions are pinned in `src-tauri/Cargo.toml`.)
- [Svelte](https://svelte.dev/) and [SvelteKit](https://svelte.dev/docs/kit)
  power the frontend, with [Vite](https://vite.dev/),
  [TypeScript](https://www.typescriptlang.org/), and
  [marked](https://github.com/markedjs/marked).

## Local machine-learning stack

- [MLX](https://github.com/ml-explore/mlx) provides Apple Silicon-native array
  and model execution.
- [mlx-lm](https://github.com/ml-explore/mlx-lm) provides local language-model
  loading and generation.
- [mlx-audio](https://github.com/Blaizzy/mlx-audio) provides the speech-to-text
  interface used by Gist.
- [Parakeet TDT 0.6B v3 MLX 4-bit](https://huggingface.co/animaslabs/parakeet-tdt-0.6b-v3-mlx-4bit),
  provided by animaslabs, is used for local transcription under the upstream
  model terms.
- [pyannote Community-1](https://huggingface.co/pyannote/speaker-diarization-community-1)
  provides local speaker diarization. See the model repository and
  [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for model attribution and
  CC BY 4.0 details.
- [Qwen 3.5](https://huggingface.co/Qwen) models, distributed for MLX through
  the [mlx-community](https://huggingface.co/mlx-community) repositories,
  provide local note generation. Gist currently references the Qwen 3.5 4B
  and 9B OptiQ 4-bit repositories in `gist/models.py`.
- [Hugging Face Hub](https://github.com/huggingface/huggingface_hub) handles
  first-run model downloads and local cache management.

## Python and packaging libraries

The sidecar uses [Click](https://click.palletsprojects.com/),
[Transformers](https://github.com/huggingface/transformers),
[tqdm](https://github.com/tqdm/tqdm),
[miniaudio](https://github.com/irmen/pyminiaudio),
[cffi](https://cffi.readthedocs.io/), and
[PyInstaller](https://pyinstaller.org/) alongside the machine-learning
dependencies above. Transitive packages such as PyTorch, TorchAudio,
TorchCodec, tokenizers, NumPy, SciPy, sentencepiece, and related runtime
components are included according to their upstream licenses when building the
sidecar.

## No adapted snippets identified

The repository audit did not identify application code copied from a specific
open-source example. Upstream project links are included here for the
frameworks, libraries, models, and build tooling used by Gist.

## Distribution note

Model files are intentionally kept outside Git and copied into the macOS app
at build time. A bundled release must retain the relevant upstream notices and
comply with the terms of every dependency and model it contains.

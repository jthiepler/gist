# Gist

> Privacy-first clinical note-taking for therapists. 100% on-device.

Gist is a free, open-source macOS app that helps therapists and mental-health
professionals turn session recordings into structured clinical notes. Audio,
transcription, speaker diarization, and note generation are designed to run
locally on Apple Silicon, so your clinical data can stay on your Mac.

<!-- Screenshot to be added later: add a 1200px-wide app screenshot or product mockup here. -->

## Why Gist exists

Therapists spend too much of their limited time turning session memories and
rough notes into compliant progress notes. Gist was built from a simple,
altruistic idea: useful clinical documentation assistance should not require
clinicians to upload intimate conversations to an unrelated cloud service or
pay another subscription. The project is early, practical, and open to input
from the people who do this work every day.

## Privacy promise

> **In the normal bundled workflow, session audio, transcripts, notes, and
> client data never leave your device.** Gist has no accounts, cloud sync,
> telemetry, or subscription. Processing uses local models on your Mac, and
> the app continues to work without internet access after its model assets
> have been downloaded.

The codebase also includes an optional OpenAI-compatible backend for local
development and advanced setups. Configure it only if you understand where
that endpoint sends data; the bundled local workflow does not require it.

## Features

- Record therapy sessions locally from the microphone.
- Pause and resume recordings.
- Transcribe speech with Parakeet TDT through `mlx-audio`, optimized for Apple
  Silicon.
- Identify speakers with local `pyannote` diarization.
- Generate concise, structured notes from session transcripts.
- Choose SOAP, DAP, BIRP, GIRP, PIRP, SIRP, DART, CBT, or intake formats.
- Create custom note templates with user-defined prompts.
- Manage clients and browse session history.
- Review an editable draft beside its source transcript.
- Export notes as plain text.
- Download and manage local note-generation models.
- Keep the entire workflow local after the initial model download.

## Requirements

- Apple Silicon Mac (M1 or later)
- macOS 14 or later
- 8 GB RAM minimum
- 16 GB RAM recommended for the Qwen 3.5 9B model
- Microphone access for recording sessions

Gist is currently a macOS-only application. The first model download requires
an internet connection and enough local disk space; later transcription and
note generation can run offline.

## Installation

1. Download the latest `Gist_*.dmg` from [GitHub Releases](../../releases).
2. Open the downloaded DMG and drag **Gist** to **Applications**.
3. Because early releases are unsigned, right-click **Gist**, choose **Open**,
   and confirm the macOS warning on first launch.
4. Grant microphone access when macOS asks. Download a local note-generation
   model when prompted or from Settings.

## Quick start

1. Create a client in Gist.
2. Start a session, choose the desired inputs, and record; pause or resume as
   needed.
3. Stop the recording and let Gist transcribe it and identify speakers.
4. Choose a note format, generate the note, edit the draft beside the
   transcript, and export it as plain text.

Review every generated note against the source material before using it in a
clinical record. Gist is documentation assistance, not a diagnostic or
clinical decision-making system.

## Note formats

<details>
<summary>Supported formats</summary>

| Format | Sections | Typical use |
| --- | --- | --- |
| SOAP | Subjective, Objective, Assessment, Plan | Standard progress notes separating client report, observations, synthesis, and next steps. |
| DAP | Data, Assessment, Plan | A compact format combining report and observations in the Data section. |
| BIRP | Behavior, Intervention, Response, Plan | Documents the presenting behavior, care delivered, response, and follow-up. |
| GIRP | Goal, Intervention, Response, Plan | Keeps the note centered on a documented treatment goal. |
| PIRP | Problem, Intervention, Response, Plan | Focuses on one primary problem addressed during the session. |
| SIRP | Situation, Intervention, Response, Plan | Organizes the current situation, intervention, client response, and plan. |
| DART | Description, Assessment, Response, Treatment | Separates factual session description from clinical assessment and treatment. |
| CBT | Session Overview, Cognitive Conceptualization, Behavioral Interventions, Cognitive Interventions, Progress and Plan | Captures explicitly documented cognitive-behavioral work. |
| Intake | Presenting Problem, Relevant History and Context, Mental Status, Risk Assessment, Clinical Impressions, Initial Plan | Structures a comprehensive first-session evaluation. |

Gist is instructed to use only information supported by the source materials.
When information is missing, it should say so rather than inventing symptoms,
diagnoses, risk findings, or treatment details.

</details>

## Built with

- [Tauri 2](https://v2.tauri.app/) and [Rust](https://www.rust-lang.org/)
- [Svelte](https://svelte.dev/), [SvelteKit](https://svelte.dev/docs/kit),
  [Vite](https://vite.dev/), and TypeScript
- [MLX](https://github.com/ml-explore/mlx) and [mlx-lm](https://github.com/ml-explore/mlx-lm)
  for Apple Silicon inference
- [mlx-audio](https://github.com/Blaizzy/mlx-audio) with
  [Parakeet TDT](https://huggingface.co/animaslabs/parakeet-tdt-0.6b-v3-mlx-4bit)
  for transcription
- [pyannote Community-1](https://huggingface.co/pyannote/speaker-diarization-community-1)
  for speaker diarization
- [Qwen 3.5](https://huggingface.co/Qwen) MLX community models for local note
  generation
- Python, SQLite, JSON-RPC, and PyInstaller

See [CREDITS.md](CREDITS.md) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
for dependency, model, and license acknowledgments.

## Development

Clone the repository and install the frontend and Python dependencies:

```bash
npm install
uv sync
```

For a local sidecar smoke test:

```bash
uv run gist formats
```

For a macOS application build, provide the two local model checkouts expected
by the build script, then build the sidecar and Tauri app:

```text
parakeet-tdt-0.6b-v3-mlx-4bit/
speaker-diarization-community-1/
```

```bash
bash scripts/build-macos.sh
npm run tauri build
```

The build creates an unsigned Apple Silicon DMG at
`src-tauri/target/release/bundle/dmg/`. The model checkouts, PyInstaller
output, Tauri resources, and Rust build artifacts are intentionally ignored
by Git.

Useful checks:

```bash
npm run check
cargo check --manifest-path src-tauri/Cargo.toml
```

## Project status

Gist is **v0.1 beta**. It is looking for early feedback from therapists,
counselors, psychologists, psychiatrists, social workers, and developers who
care about private clinical tooling. Expect rough edges and verify output
carefully before relying on it in practice.

## Contributing and feedback

Feedback, bug reports, documentation improvements, and code contributions are
welcome through [GitHub Issues](../../issues). Please do not include real
patient information, recordings, transcripts, or other protected health
information in issues or pull requests. Use synthetic examples instead.

## License

Gist is released under the [MIT License](LICENSE). Some model files and
third-party packages have their own terms; consult [CREDITS.md](CREDITS.md)
and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) before redistributing a
build.

## Suggested GitHub setup

- Recommended repository name: `gist` (use `gist-notes` if the shorter name
  is unavailable).
- Suggested topics: `therapy`, `clinical-notes`, `soap-notes`, `tauri`,
  `macos`, `rust`, `python`, `mlx`, `privacy-first`, `apple-silicon`.
- Add a `social_preview.png` at 1200×630 pixels once an app screenshot or
  mockup is available, then set it under **Settings → General → Social
  preview**.

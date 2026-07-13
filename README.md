# Gist

**Turn a therapy session into an editable clinical note without sending the
conversation to the cloud.**

Gist is a free, open-source macOS app for solo therapists and private
practitioners. It records or imports session material, transcribes it, and
creates a structured first draft in the note format you use. In the bundled
workflow, the recording, transcript, client record, and generated note remain
on your Mac.

> Gist is an early beta. It has not yet been validated in clinical practice.
> Treat every generated note as a draft: review it against the source before
> putting it in a clinical record.

<img width="2016" height="1151" alt="Gist showing a generated clinical note beside the synthetic transcript it was based on" src="docs/assets/split-review.png" />

## Why I built it

Most clinical documentation assistants are subscription services, and many
depend on sending sensitive conversations to someone else's infrastructure.
For an independent practitioner, that can mean another recurring bill and
another company that must be trusted with deeply private material.

I studied Economics and Psychology before becoming a machine-learning
engineer. I have since built ML systems in finance and legal research—two other
fields where plausible but incorrect output is not good enough. Gist brings
those parts of my background together: a deliberately local tool that makes a
routine task easier without trying to replace clinical judgment.

This is a personal open-source project, not a clinical platform or a startup
sales funnel. My aim is to make it polished enough to be genuinely useful,
then shape it through honest feedback from working therapists.

— [Joshua Hiepler](https://jthiepler.com)

## What the workflow looks like

1. Add a client and start a session.
2. Record with the microphone, import audio, paste a transcript, or add your
   own written observations.
3. Let Gist transcribe the audio and separate speakers locally.
4. Generate one or more structured drafts.
5. Review the draft beside its source, edit it, and export it as plain text.

<table>
  <tr>
    <td width="50%"><img alt="A synthetic session transcript in Gist" src="docs/assets/session-transcript.png" /></td>
    <td width="50%"><img alt="Local model selection in Gist settings" src="docs/assets/local-models.png" /></td>
  </tr>
  <tr>
    <td><em>Keep the source material with the session.</em></td>
    <td><em>Choose and manage the note-writing model on your Mac.</em></td>
  </tr>
</table>

All names and clinical material shown in these screenshots are synthetic.

## Local means local

In the normal bundled workflow:

- Gist has no account, cloud sync, telemetry, or subscription.
- Client records are stored in a local SQLite database.
- Audio transcription runs with Parakeet TDT through `mlx-audio`.
- Speaker diarization runs with `pyannote` Community-1.
- Note generation runs with a Qwen 3.5 MLX model you download and manage.
- The app can continue working offline once its model assets are present.

The initial model downloads require an internet connection. The codebase also
contains an optional OpenAI-compatible backend for development and advanced
setups; it is not required by the bundled app. If you configure it, you are
responsible for understanding where that endpoint sends data.

Local processing reduces the number of parties and systems that handle
clinical data. It does **not**, by itself, make a clinician or practice
compliant with HIPAA or any other regulation. Device security, access control,
backups, consent, retention, and the way the app is used remain the
practitioner's responsibility.

## What works today

- Local recording with pause and resume
- Audio import, pasted transcripts, and clinician-written source material
- On-device transcription and speaker diarization
- Editable notes shown beside their supporting transcript
- SOAP, DAP, BIRP, GIRP, PIRP, SIRP, DART, CBT, and intake formats
- Custom note templates and prompts
- Local client and session history
- Plain-text export
- Downloadable 4B and 9B local note-writing models

Transcription is already useful on clear audio. Speaker diarization is less
consistent and remains one of the rougher parts of the beta. Generated note
quality depends on the recording, transcript, selected model, and template;
Gist deliberately presents every output as a draft.

## Requirements

- Apple Silicon Mac (M1 or later)
- macOS 14 or later
- 8 GB RAM minimum
- 16 GB RAM recommended for the Qwen 3.5 9B model
- Roughly 1 GB for the app and bundled speech models
- An additional 2.5 GB for the smallest note-writing model, or 5.5 GB for the
  9B model
- Microphone access when recording directly in Gist

Gist is currently macOS-only.

## Install the beta

1. Download `Gist_*.dmg` from the
   [v0.1.0 beta release](https://github.com/jthiepler/gist/releases/tag/v0.1.0).
2. Open the DMG and drag **Gist** to **Applications**.
3. The beta is not yet notarized. On first launch, right-click **Gist**, choose
   **Open**, and confirm the macOS warning.
4. Grant microphone access if you plan to record sessions, then download a
   note-writing model when prompted.

If something is confusing or breaks, please
[open an issue](https://github.com/jthiepler/gist/issues). Therapists who would
rather not use GitHub can [share structured feedback](https://tally.so/r/EkEWVo)
or write to [gist@jthiepler.com](mailto:gist@jthiepler.com). Do not send real
patient information, recordings, or transcripts through any of these channels.

## Note formats

| Format | Sections |
| --- | --- |
| SOAP | Subjective, Objective, Assessment, Plan |
| DAP | Data, Assessment, Plan |
| BIRP | Behavior, Intervention, Response, Plan |
| GIRP | Goal, Intervention, Response, Plan |
| PIRP | Problem, Intervention, Response, Plan |
| SIRP | Situation, Intervention, Response, Plan |
| DART | Description, Assessment, Response, Treatment |
| CBT | Session Overview, Cognitive Conceptualization, Behavioral Interventions, Cognitive Interventions, Progress and Plan |
| Intake | Presenting Problem, Relevant History and Context, Mental Status, Risk Assessment, Clinical Impressions, Initial Plan |

Templates instruct the model to stay within the source material and state when
information is missing. That guardrail is useful, but it is not infallible.

## Build it locally

Gist has a SvelteKit frontend, a Tauri/Rust desktop layer, and a Python
JSON-RPC sidecar for inference. Install the frontend and Python dependencies:

```bash
npm install
uv sync
```

Run the routine checks:

```bash
npm run check
cargo check --manifest-path src-tauri/Cargo.toml
uv run gist formats
```

Building the distributable app requires local checkouts of the Parakeet and
pyannote models expected by `scripts/build-macos.sh`:

```text
parakeet-tdt-0.6b-v3-mlx-4bit/
speaker-diarization-community-1/
```

```bash
bash scripts/build-macos.sh
npm run tauri:dmg
```

The unsigned Apple Silicon DMG is written to
`src-tauri/target/release/bundle/dmg/`. Model checkouts, generated resources,
PyInstaller output, and Rust build artifacts are ignored by Git.

### Releases and automatic updates

Gist checks the published GitHub Releases feed in the background and can
download and install a signed update from within the app. Release builds must
include the DMG for manual installation and the generated updater files
(`latest.json`, the `.tar.gz` updater artifact, and its `.sig` signature).

Create an updater signing key once and keep the private key outside the
repository:

```bash
npx tauri signer generate --write-keys ~/.tauri/gist-updater.key
```

The public key is stored in `src-tauri/tauri.conf.json`; if you generate a
different key, replace the configured public key with the new one. Set
`TAURI_SIGNING_PRIVATE_KEY_PATH` (and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the key is password-protected), then
run `npm run tauri:release`. Attach the files listed under “Updater files” by
the script to the published GitHub Release. The release must be published,
not left as a draft, for the app’s `releases/latest/download/latest.json`
endpoint to resolve.

If no key path or environment variable is set, `npm run tauri:release` prompts
for the private key with terminal input hidden, then prompts separately for the
key’s password. Leave the updater-key password blank only if the key was created
without one. The Apple app-specific password is a separate prompt.

Each release run clears and recreates the root-level `release/` folder with the
DMG, `latest.json`, updater archive, and signature files ready to upload to
GitHub. The folder is ignored by Git and does not affect ordinary development
build output.

## Built with

[Tauri 2](https://v2.tauri.app/) · [SvelteKit](https://svelte.dev/) ·
[MLX](https://github.com/ml-explore/mlx) ·
[mlx-audio](https://github.com/Blaizzy/mlx-audio) ·
[Parakeet TDT](https://huggingface.co/animaslabs/parakeet-tdt-0.6b-v3-mlx-4bit) ·
[pyannote Community-1](https://huggingface.co/pyannote/speaker-diarization-community-1) ·
[Qwen 3.5](https://huggingface.co/Qwen) · Python · Rust · SQLite

See [CREDITS.md](CREDITS.md) and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for model, dependency, and
license acknowledgements.

## Contributing

The most useful contribution right now is candid feedback from therapists and
private practitioners who have tried the workflow. You can complete the
[therapist feedback form](https://tally.so/r/EkEWVo), email
[gist@jthiepler.com](mailto:gist@jthiepler.com), or use
[GitHub Issues](https://github.com/jthiepler/gist/issues) for technical reports.
Documentation improvements and focused code contributions are also welcome.

Please never include protected health information or real client material in
an issue, discussion, commit, or pull request.

## License

Gist is released under the [MIT License](LICENSE). Bundled models and some
third-party packages have their own terms; review the credits and notices
before redistributing a build.

# Repository Guidelines for Coding Agents

This file is the operational entry point for work in Gist. Follow it for every
change. More specific contracts linked below are mandatory within their scope.

## Product and engineering priorities

Gist is a local-first macOS application for sensitive therapy-session records.
When priorities conflict, optimize in this order:

1. Preserve clinical data and historical meaning.
2. Protect privacy and keep processing local.
3. Fail safely and recoverably.
4. Maintain compatibility with supported stored data and backups.
5. Keep behavior clear, testable, and maintainable.
6. Improve usability and performance without weakening the above.

Never treat storage, retention, restore, or deletion as ordinary CRUD. A small
change in those areas can affect records users may be legally required to keep
for ten years.

## Mandatory data-lifecycle contract

Before changing persistence, migrations, patient/session/source/note data,
recordings, uploaded audio, recovery jobs, backups, restore, archives,
encryption, templates, or pipeline representations, read
[`DATA_LIFECYCLE.md`](DATA_LIFECYCLE.md) completely.

Its invariants are part of the application contract. In particular:

- SQLite is the canonical durable library.
- Stored clinical artifacts are preserved, not regenerated during migration.
- Released migrations and backup readers are append-only compatibility code.
- New Gist versions migrate supported old backups; old versions reject unknown
  newer schemas and formats.
- Restorable backups and human-readable archives remain separate formats.
- Human-readable archives contain only plainly organized `.txt` documents;
  never expose UUIDs, model metadata, RPC shapes, or database-oriented files in
  them.
- Recorded audio is transient; uploaded audio is user-owned and untouched.
- Audio, recovery jobs, caches, diagnostics, models, logs, and application
  settings stay out of user exports. Customized built-in template derivatives,
  completely custom templates, patient template preferences, and record-level
  model provenance remain durable.
- Note history is immutable and ordered by explicit revision numbers.
- Restore validates in staging and creates a rollback before activation.

If a requested implementation conflicts with that contract, do not quietly
work around it. Surface the conflict and make the product decision explicit.

## Architecture

Gist is a Tauri 2 macOS desktop application with three runtime layers:

- `src/` — SvelteKit SPA. Routes live in `src/routes/`, shared TypeScript in
  `src/lib/`, and reusable UI in `src/lib/components/`.
- `src-tauri/` — Rust host. It owns SQLite, Tauri commands, file dialogs,
  backup/restore, recording lifecycle, windows, and sidecar lifecycle.
- `gist/` — Python CLI and JSON-RPC sidecar. It owns transcription, diarization,
  evidence extraction, note generation, and model backends.

Supporting areas:

- `tests/` — model-independent Python and Node tests.
- `scripts/` — packaging, versioning, sidecar, and release automation.
- `static/`, `icon/` — application assets.
- `docs/` — public website content, not canonical engineering documentation
  unless explicitly linked from this file.
- `DATA_LIFECYCLE.md` — canonical data and compatibility architecture.

Respect layer ownership:

- Frontend code orchestrates UX but is not a privacy or integrity boundary.
- Rust validates persistence, paths, retention, restore, and destructive
  operations even when the frontend already checks them.
- Python processing receives explicit inputs and returns artifacts; it must not
  become an undeclared source of durable state.
- UI state, caches, and downloaded models must not become prerequisites for
  migration or restore.

## Start every task by orienting

Before editing:

1. Read this file and any mandatory scoped document.
2. Inspect `git status` and preserve unrelated user changes.
3. Locate all callers and consumers of the behavior being changed.
4. Identify which layer owns the invariant, not merely where the symptom is
   visible.
5. Classify new data as durable, transient, reconstructable, or diagnostic.
6. Decide how the change will be verified without downloading models.

Prefer focused changes, but refactor adjacent code when necessary to keep one
clear source of truth, enforce an invariant at the correct boundary, or remove
duplication exposed by the work.

## Development setup and commands

Install dependencies with:

```bash
npm install
uv sync
```

Common development commands:

```bash
npm run dev
npm run tauri dev
uv run gist formats
```

The browser-only Vite app cannot exercise native Tauri commands. Use it for
layout and non-native frontend work. Use `npm run tauri dev` for file dialogs,
recording, restore, relaunch, menus, and other native behavior.

Routine validation:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run check
npm test
npm run build
uv run python -m unittest discover tests
git diff --check
```

Version consistency:

```bash
npm run check-version
```

Full packaging is slower and may require local model resources:

```bash
bash scripts/build-macos.sh
npm run tauri build
```

Do not make routine tests depend on model downloads, network access, signing
credentials, notarization, or the local diarization checkout.

## Validation expectations by change type

Run checks in proportion to the change, with these minimums.

### Frontend-only behavior

- `npm run check`
- `npm test`
- `npm run build` for routing, bundling, or dependency changes
- Manual verification for affected UI behavior

### Rust or Tauri behavior

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `npm run check` when Tauri command types or frontend calls change
- Manual Tauri verification for native flows when practical

### Python sidecar behavior

- `uv run python -m unittest discover tests`
- A focused CLI or JSON-RPC smoke test for the changed operation
- No model download for ordinary validation

### Storage, backup, restore, archive, or retention

- All Rust checks above
- Relevant frontend checks
- Plain and encrypted backup round trips
- Old-schema or old-format fixture migration when compatibility changes
- Negative tests for corruption, unsupported versions, unsafe ZIP content, and
  rollback behavior as applicable
- Confirmation that excluded transient data is absent
- `DATA_LIFECYCLE.md` and user-facing documentation review

### Release or packaging

- `npm run check-version`
- Full frontend, Rust, and Python suites
- Appropriate macOS bundle build
- Review permissions, resources, updater artifacts, signing, and notices

If a required check cannot run, report exactly what was not verified and why.
Do not describe an unrun check as passing.

## Coding conventions

### General

- Prefer plain, explicit code over clever abstractions.
- Keep one source of truth for shared classifications and constants.
- Remove dead code and avoid speculative abstractions without a concrete use.
- Preserve comments that explain invariants or non-obvious failure ordering;
  do not narrate obvious syntax.
- Return actionable errors at system boundaries. Do not silently swallow data,
  persistence, encryption, or deletion failures.
- Best-effort cleanup may log and continue only when the primary durable action
  has succeeded and retry remains possible.
- Bound untrusted input by count, size, path, and version before allocating or
  writing large data.
- Use atomic writes and private permissions for clinical-data files.

### TypeScript and Svelte

- Use two-space indentation and `camelCase` identifiers.
- Use `PascalCase` for components and exported types.
- Put reusable domain logic in `src/lib/`, not inside large components.
- Use shared RPC and domain types rather than duplicating payload shapes.
- Treat frontend validation as UX; repeat security and integrity validation in
  Rust.
- Clean up subscriptions, timers, and listeners during component destruction.
- Preserve accessibility labels, keyboard and focus behavior, and disabled or
  busy states when changing controls.

### Rust

- Use `snake_case` identifiers and standard Rust formatting.
- Keep database mutations transactional when multiple statements form one
  logical operation.
- Check affected-row counts when absence would violate the operation.
- Validate filesystem ownership and exact targets before deletion.
- Avoid holding application state locks across `.await` points.
- Do not use `unwrap`, `expect`, or panic for user-controlled runtime input.
  Bundled compile-time invariants and tests are exceptions.
- Put focused model-independent tests beside the relevant module.

### Python

- Use four-space indentation and `snake_case` identifiers.
- Keep JSON-RPC request and response behavior explicit and serializable.
- Separate backend/model adapters from stable pipeline representations.
- Do not write patient content, transcripts, or model output to logs.
- Preserve deterministic model-free tests for parsing, formatting, and
  orchestration behavior.

### SQL and migrations

- Parameterize values; do not interpolate user data into SQL.
- Dynamic identifiers are acceptable only from closed, code-owned constants.
- Enable and validate foreign keys.
- Use explicit schema versions and transactional, forward-only migrations.
- Never change a released migration to make a new test pass; add a new version.
- Backfills must be deterministic and independent of models or network access.
- See `DATA_LIFECYCLE.md` for the complete migration playbook.

## Privacy, security, and destructive operations

Gist handles sensitive clinical information. Treat names, transcripts, notes,
templates, metadata, diagnostics, and file paths as potentially sensitive.

- Never commit recordings, real transcripts, patient records, generated
  backups, readable archives, credentials, downloaded models, or sidecar
  bundles.
- Never upload or transmit clinical content through diagnostics, tests, or
  development tooling.
- Never broaden telemetry or logging without an explicit privacy decision.
- Never delete an uploaded user file. Gist only deletes files it created and
  can prove are in an app-owned location with a recognized managed name.
- Resolve and validate destructive targets before acting. Prefer staged,
  atomic, or recoverable operations.
- Sanitize temporary clinical data and ensure crash leftovers are private and
  bounded.
- Do not call an unencrypted checksum authentication or proof against malicious
  tampering.
- Review `THIRD_PARTY_NOTICES.md` when changing bundled dependencies.

If a change affects macOS permissions, sandbox behavior, microphone access,
file access, encryption, retention, or external communication, call it out in
the handoff and pull request.

## Data and API evolution

When changing a Tauri command or JSON-RPC message:

1. Update the producer, consumer, and shared types together.
2. Search for every caller and event listener.
3. Decide whether old persisted payloads or in-progress recovery jobs can still
   be encountered.
4. Prefer additive fields with safe defaults where compatibility matters.
5. Add a migration for durable representation changes.
6. Keep user-facing archives independent from internal RPC shapes.

Do not use application version numbers as a substitute for a schema,
container, archive, or pipeline format version.

## Documentation requirements

Update documentation in the same change when behavior or guarantees change:

- `DATA_LIFECYCLE.md` for storage, compatibility, retention, restore, or
  archive engineering contracts
- `README.md` for user-visible capabilities, privacy, setup, or contribution
  guidance
- `docs/index.html` for public website claims
- Onboarding and Settings copy for in-app behavior
- `THIRD_PARTY_NOTICES.md` for relevant dependency changes

Keep one canonical detailed explanation and link to it elsewhere. Avoid copying
large sections that will drift independently.

## Generated files and repository hygiene

- Preserve unrelated modifications in a dirty worktree.
- Do not stage, commit, push, or open a pull request unless requested.
- Do not edit generated build output by hand.
- Keep generated sidecars, model files, release bundles, backups, archives, and
  clinical test artifacts out of version control.
- Update lockfiles when dependency manifests change.
- Check `git diff --check` before handoff.
- Review final status and diff for accidental or unrelated files.

## Commit and pull request guidance

Use short imperative subjects with familiar prefixes such as `feat:`, `fix:`,
`refactor:`, `test:`, and `docs:`.

Pull requests should include:

- The user-visible and architectural outcome
- Important invariants or privacy decisions
- Migration and compatibility impact
- Validation commands and results
- Screenshots for UI changes
- macOS permission or bundled-resource changes
- Known limitations or intentionally deferred work

Never claim backward compatibility without exercising an old fixture or
explaining the exact compatibility mechanism.

## Definition of done

A change is complete when:

- The requested behavior is implemented at the correct layer.
- Integrity and privacy invariants are enforced in Rust or the owning backend,
  not only in the UI.
- Duplication and obsolete paths introduced or exposed by the work are removed.
- Relevant migrations and compatibility readers remain intact.
- Focused regression tests cover important success and failure paths.
- Required checks pass, or unverified items are explicitly reported.
- User-facing and engineering documentation agree with the implementation.
- The final diff contains no accidental sensitive, generated, or unrelated
  files.
- The handoff states what changed, what was validated, and any manual checks
  still expected from the user.

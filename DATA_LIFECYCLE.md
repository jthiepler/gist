# Gist Data Lifecycle and Compatibility Contract

This document is the normative engineering guide for durable clinical data,
transient media, backups, restores, human-readable archives, and processing
pipeline evolution in Gist. It is written for maintainers and coding agents.

Read this document before changing any of the following:

- SQLite tables, columns, indexes, constraints, or migrations
- Patient, session, source, note, template, or settings persistence
- Note revision behavior
- Recording, uploaded-audio, recovery-job, or retention behavior
- Backup manifests, ZIP contents, encryption, restore, or rollback behavior
- Human-readable archive paths or file formats
- Transcript, evidence, or note-generation representations
- Model, prompt, template, or pipeline metadata stored with clinical records

The code is the source of truth for current version numbers. The relevant
constants are `LATEST_DATABASE_SCHEMA_VERSION` in `src-tauri/src/lib.rs` and
`BACKUP_FORMAT_VERSION` in `src-tauri/src/data_management.rs`.

## Non-negotiable invariants

1. SQLite is the canonical durable library. UI state and generated archive
   layouts are not canonical storage.
2. Stored clinical artifacts are historical facts. Migrations preserve them;
   they do not silently regenerate or reinterpret them with a newer pipeline.
3. New applications must remain able to restore supported old backups by
   migrating their contained database forward.
4. Old applications must reject unknown newer schemas or backup formats. They
   must never guess, partially import, or silently discard unknown data.
5. Restorable backups and human-readable archives are separate products. A
   human archive is not an import format.
6. Recorded audio is transient. Uploaded audio remains user-owned and Gist
   neither copies nor deletes it. Neither kind belongs in a durable export.
7. Recovery material has a bounded lifetime. It must not become durable merely
   because its metadata happens to be stored temporarily in SQLite.
8. Notes have immutable history. A changed note appends a revision; an
   identical save does not manufacture a revision.
9. Every restore is validated in staging before the active library is touched.
   The current library is snapshotted before replacement and restored
   automatically if activation fails.
10. Unencrypted checksums detect accidental corruption, not malicious
    modification. Authenticated `age` encryption provides confidentiality and
    tamper detection.
11. Diagnostics, caches, models, logs, recovery jobs, audio, and retained audio
    paths are excluded from durable user exports.
12. Tests and fixtures contain synthetic data only. Never commit patient data,
    real transcripts, recordings, credentials, or generated clinical bundles.

If a proposed feature conflicts with an invariant, stop and redesign it or
explicitly revise this contract as part of a reviewed product decision.

## Compatibility layers

Treat these as independent contracts:

| Layer | Version authority | Purpose |
| --- | --- | --- |
| Live SQLite schema | `LATEST_DATABASE_SCHEMA_VERSION` | Current canonical storage |
| Restorable backup container | `BACKUP_FORMAT_VERSION` | Packaging, manifest, and payload rules |
| Human-readable archive | Human-facing folder layout | Long-term access without Gist |
| Processing pipeline | Stored model/source/pipeline metadata | Provenance of generated artifacts |

A database migration usually does not require a new backup-container version.
The backup already declares the schema version of its SQLite payload. Bump the
backup format only when the outer contract changes, such as its manifest,
payload structure, signature scheme, or attachment model.

Compatibility intentionally works in this direction:

| Combination | Expected behavior |
| --- | --- |
| New Gist + supported old backup | Validate, migrate, restore |
| Current Gist + current backup | Validate and restore |
| Old Gist + newer schema or format | Reject with a clear upgrade-required error |
| Human archive + no Gist installation | Remain readable with ordinary tools |

There is no safe general mechanism for an old binary to interpret arbitrary
future data. Rejection is preferable to partial restoration of clinical data.

## Durable and transient data

The durable written library includes:

- Patients and sessions
- Transcript and clinician-authored source text
- Current notes and immutable note revisions
- Customized built-in template derivatives, completely custom note templates,
  and patient template preferences
- Model, template, and pipeline provenance stored with written records

The following are transient or reconstructable and do not belong in exports:

- Recorded audio and temporary audio paths
- Uploaded-audio paths or copies
- Recording recovery jobs
- Evidence caches and other derived caches
- Downloaded models
- Application settings, including model selection, onboarding, appearance,
  menu-bar behavior, and feedback state
- Logs and developer diagnostics
- Build artifacts and sidecar runtime files

When adding a new table or field, explicitly classify it as durable,
transient, reconstructable, or diagnostic. Then update backup sanitation,
archive projection, restore validation, tests, and this document as required.
Do not allow its classification to emerge accidentally from the fact that it
lives in SQLite.

Bundled template definitions are application-owned and may be synchronized
from the current catalog. A user edit to a bundled template must be represented
as a separate durable custom row; it must never be folded into a generic
setting or overwritten by catalog synchronization. Fully custom templates use
the same durable representation. Patient template selections belong in
`patient_note_formats`, not in `settings`.

## Live SQLite and immutable notes

`session_notes` is the materialized current note used by the application.
`note_revisions` is the immutable historical record.

For each note:

- Revision numbers begin at 1 and are contiguous.
- `(note_id, revision_number)` is unique.
- Revision 1 has no predecessor.
- Every later revision references the immediately preceding revision.
- The current note content equals the latest revision content.
- Saving identical content and model metadata is a no-op.

Do not rely on SQLite `rowid`, local-time string ordering, or UUID ordering for
historical sequence. Compaction can change row IDs, clock offsets change over
time, and UUIDs are deliberately unordered.

When provenance requirements expand, prefer additive revision metadata such
as pipeline version, template revision or prompt snapshot, source fingerprint,
generation parameters, authorship, finalization, and amendment reason. Do not
make restore depend on the continued availability of the original AI model.

## Audio and recovery lifecycle

For an in-app recording:

1. Create a recovery job and Gist-owned temporary recording.
2. Transcribe the recording locally.
3. Commit the transcript as durable source text.
4. Clear the temporary audio reference and delete the recording.
5. Generate or update notes from the stored transcript.

Deleting audio after transcript commit, rather than after note generation,
keeps audio lifetime independent from a later model failure.

If the app stops before cleanup, the recovery job and recording may survive
for the configured recovery period. Expiry is measured from original creation,
not from the most recent retry. Only UUID-named files created in Gist's
recordings directory are eligible for managed deletion.

For uploaded audio:

- Read the original in place.
- Do not copy it into application storage.
- Do not persist its path.
- Do not delete or modify it.

Backend validation must enforce these rules. Frontend behavior alone is not a
privacy boundary.

## Restorable backup contract

A restorable backup is a sanitized, transactionally consistent SQLite snapshot
inside a versioned ZIP container. The current format includes:

- `manifest.json`
- `library.sqlite3`
- `SHA256SUMS`
- `README.txt`

Before packaging, the snapshot is sanitized:

- Remove recording recovery jobs.
- Clear all retained audio references.
- Remove evidence and derived caches.
- Remove all application settings. Model and template provenance attached to
  written records remains part of the durable library.
- Enable SQLite secure deletion.
- Compact the snapshot so deleted transient values do not remain in free pages.
- Run integrity, foreign-key, identifier, and note-revision validation.

Restore migrates and sanitizes the staged backup before activation. Settings
from an older backup are discarded, and the destination installation's current
settings are copied into the staged library. Internal pre-restore rollback
snapshots preserve those local settings because their purpose is to return the
same installation to its exact pre-restore state, not to provide portability.

The manifest declares at least the container format, schema version, creator
version, creation time, database filename, SHA-256 checksum, record counts, and
whether prohibited attachments are present.

ZIP creation is atomic, uses private file permissions, supports ZIP64, and
syncs completed data before the temporary file is renamed into place. Optional
encryption wraps the complete ZIP using authenticated `age` encryption. Preserve
passphrase bytes exactly; whitespace must not be silently trimmed. Strength
rules for creating a new export must not prevent reading an older export that
used a weaker passphrase.

Exports may not target Gist's private application-data directory. Plain and
encrypted input files are size-bounded before parsing or decryption. Restore
requires exactly the recognized container entries, validates the checksum file
against the manifest, rejects unknown manifest fields and unsafe database
objects, and validates the complete current table, column, relationship, and
uniqueness schema after migration. Application-data and rollback directories
use owner-only permissions; clinical-data files use private file permissions.
Plaintext staging directories use a recognized app-owned prefix and are removed
on normal completion. Startup removes recognized leftovers from an interrupted
export, encryption, inspection, or restore; unrelated directories and retained
rollback snapshots are never included in that cleanup.

## Restore contract

Restore must perform all untrusted-file work before replacing the live library:

1. Detect and, when necessary, decrypt `age` input into a private temporary
   location with a strict size limit.
2. Validate the ZIP and reject traversal paths, symbolic links, duplicates,
   unknown entries, missing entries, and oversized payloads.
3. Parse and validate the manifest.
4. Verify the database checksum while extracting into staging.
5. Open the staged SQLite database and run integrity and foreign-key checks.
6. Confirm its actual schema and record counts match the manifest.
7. Reject schemas newer than the application supports.
8. Apply all required migrations in order.
9. Sanitize transient state again and validate current-schema invariants.
10. Create a private sanitized rollback snapshot of the current library.
11. Activate the staged database and validate it again.
12. If activation fails, restore the rollback automatically.
13. Retain only the configured number of rollback snapshots, clean orphaned
    transient recordings, and restart the application.

Inspection shown before the confirmation dialog may read manifest metadata,
but the actual restore must repeat authoritative validation. The file may have
changed between selection and activation.

Restore is refused while an unfinished recording recovery exists. A recovery
belongs to the current library and cannot be safely merged into the replacement
library; the practitioner must process or explicitly discard it first.

## Human-readable archive contract

The human archive is an independent projection for reading and long-term
storage. It is deliberately not accepted by restore.

It contains ordinary, open formats:

- A `Start Here.txt` guide
- A `Patients` folder with one plainly named folder per patient
- Dated, plainly named session folders
- Plain-text patient and session information, source texts, current notes, and
  note history
- A `Templates` folder containing customized built-in template derivatives and
  completely custom templates as plain text

Archive generation streams entries into ZIP rather than retaining the complete
archive in memory. Every entry ends in `.txt` and uses UTF-8 text so it opens in
ordinary writing applications. Path labels are sanitized and bounded for
extraction on common filesystems. Duplicate human labels receive simple
numeric suffixes such as `(2)`; internal UUIDs are never exposed to resolve
collisions.

The archive deliberately omits internal identifiers, model and pipeline
metadata, JSON, Markdown, CSV, HTML, checksums, and application-oriented field
names. Preserve clinically meaningful text and dates, but do not turn the
archive into a database dump. It is neither a machine-consumption contract nor
an import format. Optimize it for a non-technical practitioner navigating with
Finder and TextEdit, and test its contents as a human-facing projection.

## Schema migration playbook

For a new schema version `N`:

1. Add `migrate_to_vN` without modifying released historical migrations.
2. Run it in a SQLite transaction.
3. Add nullable fields or safe defaults before backfilling required data.
4. Backfill deterministically from stored facts; do not call AI models or
   depend on downloaded resources.
5. Add constraints and indexes only after the backfill satisfies them.
6. Set `PRAGMA user_version = N` only at the end of the transaction.
7. Increment `LATEST_DATABASE_SCHEMA_VERSION`.
8. Test upgrades from every supported prior schema fixture.
9. Test an old backup through migration and a new backup round trip.
10. Review backup sanitation, record counts, archive projection, and restore
    invariants for the new fields or tables.

Once a schema version has shipped, its migration is append-only compatibility
code. Do not edit, reorder, or delete it. For a high-risk production migration,
also provide or verify a pre-migration snapshot strategy so a semantically bad
but technically successful migration remains recoverable.

## Backup-format playbook

Do not bump the backup format merely because the SQLite schema changed.

When the container itself changes:

1. Define the new manifest and payload rules explicitly.
2. Keep the old reader and add version dispatch.
3. Write only the newest format unless there is a product reason to offer an
   older writer.
4. Preserve synthetic fixtures for every released backup format.
5. Test plain and encrypted reads for every supported version.
6. Test corrupted checksums, wrong passphrases, unknown entries, traversal,
   duplicates, oversized payloads, newer schemas, and rollback failure paths.

Never reuse a released `format_version` for incompatible semantics.

## Pipeline and representation playbook

Pipeline evolution creates new artifacts; it does not rewrite history.

If transcripts move from text to structured speaker turns, for example:

- Preserve the existing text.
- Add a structured column or child table through a migration.
- Record the representation or pipeline version.
- Maintain a readable text projection for consumers that need it.
- Export both representations where useful.
- Do not re-run old transcripts through a new model automatically.

For changes to note models, prompts, evidence extraction, or formatting:

- Create a new note revision when content changes.
- Preserve the old revision and its provenance.
- Never require model inference during migration or restore.
- Add provenance fields before claiming reproducibility that current metadata
  cannot support.

Caches may be invalidated or discarded. Durable source text, note content, and
revision history may not.

## Required tests and fixtures

The compatibility suite must grow as formats are distributed. Pre-release
schemas may be constructed programmatically in tests; once an external build
can create a schema or backup format, preserve an immutable synthetic fixture
for it. The suite should represent:

- Every supported released SQLite schema
- Every supported backup-container version
- Plain and `age`-encrypted backups
- Older encrypted backups with legacy passphrase policies
- A newer unsupported schema or container that must be rejected
- Corrupted checksum and malformed ZIP cases
- Invalid foreign keys and revision chains
- A representative human-readable archive
- A large synthetic or sparse-file stress case for streaming and ZIP64 paths

For each storage release, verify:

1. Old backups restore with the current app.
2. Migration preserves record counts and clinical content unless an explicitly
   documented migration adds derived metadata.
3. Current notes match their latest immutable revisions.
4. A new backup restores to equivalent durable records.
5. Audio, recovery jobs, caches, and diagnostics are absent.
6. The human archive contains every expected durable clinical text without
   exposing internal identifiers or implementation metadata.
7. Restore failure leaves the original library active.

Routine validation commands are listed in `AGENTS.md`. Expensive multi-gigabyte
stress cases may run outside the routine suite, but their procedure and latest
result should be recorded for storage releases. Tests must not require model
downloads or real clinical data.

## Change review checklist

Before completing a relevant change, answer all applicable questions:

- What new data is durable, transient, reconstructable, or diagnostic?
- Does a schema migration preserve every old stored artifact?
- Can the latest app restore all supported old backup fixtures?
- Will an old app reject the new unsupported schema or format cleanly?
- Are backup sanitation and restore validation updated?
- Is the human archive projection updated without becoming an accidental
  import contract?
- Could deleted transient metadata remain in SQLite free pages or temporary
  plaintext files?
- Could any path escape the app-owned directory or cause deletion of a
  user-owned file?
- Does note history remain immutable and correctly ordered?
- Does the change accidentally depend on a model, cache, UI state, or external
  service during migration or restore?
- Are checksums being described accurately rather than as authentication?
- Are old readers, migrations, and synthetic fixtures still present?
- Have privacy implications been called out in the pull request?

## Code map

- Schema versions and migrations: `src-tauri/src/lib.rs`
- Note materialization and revision appends: `src-tauri/src/lib.rs`
- Recording recovery and transient cleanup: `src-tauri/src/lib.rs`
- Backup, archive, encryption, and restore: `src-tauri/src/data_management.rs`
- Frontend commands and shared result types: `src/lib/rpc.ts`,
  `src/lib/types.ts`
- Settings UI: `src/routes/settings/+page.svelte`
- Audio-to-transcript commit ordering: `src/lib/processSession.ts`

Keep this map and the contract above current whenever responsibilities move.

//! Restorable backups, human-readable archives, encryption, and restore.
//!
//! The compatibility and privacy rules for this module are normative and live
//! in the repository's `DATA_LIFECYCLE.md`. Read that contract before changing
//! the manifest, container contents, sanitation, validation, or archive layout.

use age::secrecy::SecretString;
use chrono::{Local, Utc};
use rusqlite::{params, Connection, DatabaseName};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::iter;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{migrate_database, LATEST_DATABASE_SCHEMA_VERSION};

// Container versions are independent of SQLite schema versions. Once a format
// ships, keep its reader and add version dispatch rather than changing its
// meaning in place. See DATA_LIFECYCLE.md.
const BACKUP_FORMAT: &str = "com.gist.backup";
const BACKUP_FORMAT_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_DATABASE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_BACKUP_CONTAINER_BYTES: u64 = MAX_DATABASE_BYTES + 4 * MAX_MANIFEST_BYTES;
const MAX_ENCRYPTED_EXPORT_BYTES: u64 = MAX_BACKUP_CONTAINER_BYTES + 64 * 1024 * 1024;
const MAX_ARCHIVE_LABEL_CHARS: usize = 56;
const DATA_OPERATION_TEMP_PREFIX: &str = ".gist-data-operation-";
const RECORD_COUNT_NAMES: [&str; 7] = [
    "patients",
    "sessions",
    "session_inputs",
    "session_notes",
    "note_revisions",
    "custom_note_formats",
    "patient_note_formats",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupManifest {
    format: String,
    format_version: u32,
    created_by_gist_version: String,
    created_at: String,
    database_schema_version: i64,
    database_file: String,
    database_sha256: String,
    contains_audio: bool,
    contains_diagnostics: bool,
    record_counts: BTreeMap<String, i64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ExportResult {
    pub path: String,
    pub patient_count: i64,
    pub session_count: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RestoreResult {
    pub patient_count: i64,
    pub session_count: i64,
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn open_private_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path).map_err(|e| e.to_string())
}

fn validate_export_destination(destination: &Path, workspace: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "The selected destination has no parent directory.".to_string())?
        .canonicalize()
        .map_err(|_| "The selected destination folder is not available.".to_string())?;
    let workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
    if parent.starts_with(&workspace) {
        return Err("Choose a destination outside Gist's private application-data folder.".into());
    }
    Ok(())
}

fn remove_file_if_present(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("Could not remove an incomplete data export: {error}"),
    }
}

fn private_temp_dir(workspace: &Path) -> Result<TempDir, String> {
    tempfile::Builder::new()
        .prefix(DATA_OPERATION_TEMP_PREFIX)
        .tempdir_in(workspace)
        .map_err(|e| e.to_string())
}

pub(crate) fn cleanup_stale_data_operation_directories(workspace: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(workspace).map_err(|e| e.to_string())?;
    let mut failures = 0_usize;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.starts_with(DATA_OPERATION_TEMP_PREFIX) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        let result = if file_type.is_dir() {
            std::fs::remove_dir_all(path)
        } else if file_type.is_symlink() {
            std::fs::remove_file(path)
        } else {
            continue;
        };
        if result.is_err() {
            failures += 1;
        }
    }
    if failures == 0 {
        Ok(())
    } else {
        Err(format!(
            "Could not remove {failures} stale private data-operation directories."
        ))
    }
}

fn supplied_passphrase(passphrase: Option<&str>) -> Option<&str> {
    passphrase.filter(|value| !value.trim().is_empty())
}

fn validate_export_passphrase(passphrase: Option<&str>) -> Result<Option<&str>, String> {
    match supplied_passphrase(passphrase) {
        Some(value) if value.trim().chars().count() < 12 => {
            Err("Encrypted exports require a passphrase of at least 12 characters.".into())
        }
        value => Ok(value),
    }
}

fn encrypt_age(source: &Path, destination: &Path, passphrase: &str) -> Result<(), String> {
    let input = File::open(source).map_err(|e| e.to_string())?;
    let output = open_private_file(destination)?;
    let encryptor = age::Encryptor::with_user_passphrase(SecretString::from(passphrase.to_owned()));
    let mut writer = encryptor
        .wrap_output(output)
        .map_err(|e| format!("Could not initialize backup encryption: {e}"))?;
    let mut input = BufReader::new(input);
    std::io::copy(&mut input, &mut writer).map_err(|e| e.to_string())?;
    let output = writer
        .finish()
        .map_err(|e| format!("Could not finish backup encryption: {e}"))?;
    output.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

fn is_age_file(path: &Path) -> Result<bool, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut prefix = [0_u8; 21];
    let count = file.read(&mut prefix).map_err(|e| e.to_string())?;
    Ok(&prefix[..count] == b"age-encryption.org/v1")
}

fn decrypt_age(source: &Path, destination: &Path, passphrase: &str) -> Result<(), String> {
    let input = BufReader::new(File::open(source).map_err(|e| e.to_string())?);
    let decryptor = age::Decryptor::new_buffered(input)
        .map_err(|_| "This is not a valid encrypted Gist export.".to_string())?;
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_owned()));
    let mut reader = decryptor
        .decrypt(iter::once(&identity as &dyn age::Identity))
        .map_err(|_| "The export passphrase is incorrect or the file is damaged.".to_string())?;
    let mut output = open_private_file(destination)?;
    let mut written = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        written = written.saturating_add(count as u64);
        if written > MAX_BACKUP_CONTAINER_BYTES {
            return Err("The decrypted backup exceeds the supported size limit.".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
    }
    output.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

fn record_counts(conn: &Connection) -> Result<BTreeMap<String, i64>, String> {
    let mut counts = BTreeMap::new();
    for (name, table) in [
        ("patients", "patients"),
        ("sessions", "sessions"),
        ("session_inputs", "session_inputs"),
        ("session_notes", "session_notes"),
        ("note_revisions", "note_revisions"),
        ("custom_note_formats", "note_formats WHERE is_builtin = 0"),
        ("patient_note_formats", "patient_note_formats"),
    ] {
        let count = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|e| e.to_string())?;
        counts.insert(name.to_string(), count);
    }
    Ok(counts)
}

fn verify_database(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("PRAGMA trusted_schema = OFF;")
        .map_err(|e| e.to_string())?;
    let integrity: String = conn
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if integrity != "ok" {
        return Err(format!("SQLite integrity check failed: {integrity}"));
    }
    let foreign_key_errors: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    if foreign_key_errors != 0 {
        return Err(format!(
            "The data library contains {foreign_key_errors} invalid relationship(s)."
        ));
    }
    let unsafe_schema_objects: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type IN ('trigger', 'view')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if unsafe_schema_objects != 0 {
        return Err("The data library contains unsupported database triggers or views.".into());
    }
    for (table, column) in [
        ("patients", "id"),
        ("sessions", "id"),
        ("session_inputs", "id"),
        ("session_notes", "id"),
        ("note_revisions", "id"),
        ("note_formats", "id"),
        ("recording_jobs", "id"),
    ] {
        let mut stmt = conn
            .prepare(&format!("SELECT {column} FROM {table}"))
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if let Some(invalid) = ids.iter().find(|id| Uuid::parse_str(id).is_err()) {
            return Err(format!(
                "The data library contains an invalid identifier in {table}: {invalid}"
            ));
        }
    }
    let schema_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if schema_version >= 3 {
        let invalid_sequences: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM (
                    SELECT note_id FROM note_revisions
                    GROUP BY note_id
                    HAVING MIN(revision_number) != 1
                        OR MAX(revision_number) != COUNT(*)
                 )",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if invalid_sequences != 0 {
            return Err(format!(
                "The data library contains {invalid_sequences} invalid note revision sequence(s)."
            ));
        }
        let invalid_links: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_revisions current
                 WHERE (current.revision_number = 1 AND current.supersedes_revision_id IS NOT NULL)
                    OR (current.revision_number > 1 AND current.supersedes_revision_id IS NOT (
                        SELECT previous.id FROM note_revisions previous
                        WHERE previous.note_id = current.note_id
                          AND previous.revision_number = current.revision_number - 1
                    ))",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if invalid_links != 0 {
            return Err(format!(
                "The data library contains {invalid_links} invalid note revision link(s)."
            ));
        }
        let inconsistent_notes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_notes n
                 WHERE (n.note IS NULL AND EXISTS (
                       SELECT 1 FROM note_revisions r WHERE r.note_id = n.id
                   ))
                    OR (n.note IS NOT NULL AND NOT EXISTS (
                       SELECT 1 FROM note_revisions r
                       WHERE r.note_id = n.id
                         AND r.revision_number = (
                             SELECT MAX(latest.revision_number)
                             FROM note_revisions latest
                             WHERE latest.note_id = n.id
                         )
                         AND r.content = n.note
                         AND r.llm_model IS n.llm_model
                   ))",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if inconsistent_notes != 0 {
            return Err(format!(
                "The data library contains {inconsistent_notes} note(s) that do not match their latest immutable revision."
            ));
        }
    }
    Ok(())
}

fn table_names(conn: &Connection) -> Result<BTreeSet<String>, String> {
    let mut statement = conn
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .map_err(|e| e.to_string())?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(names)
}

fn table_columns(conn: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(columns)
}

fn foreign_keys(conn: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA foreign_key_list({table})"))
        .map_err(|e| e.to_string())?;
    let keys = statement
        .query_map([], |row| {
            Ok(format!(
                "{}:{}:{}:{}",
                row.get::<_, String>(3)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(keys)
}

fn has_unique_index(conn: &Connection, table: &str, columns: &[&str]) -> Result<bool, String> {
    let mut indexes = conn
        .prepare(&format!("PRAGMA index_list({table})"))
        .map_err(|e| e.to_string())?;
    let names = indexes
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for (name, unique) in names {
        if !unique {
            continue;
        }
        let mut info = conn
            .prepare(&format!(
                "PRAGMA index_info('{}')",
                name.replace('\'', "''")
            ))
            .map_err(|e| e.to_string())?;
        let indexed_columns = info
            .query_map([], |row| row.get::<_, String>(2))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if indexed_columns == columns {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn verify_current_database(conn: &Connection) -> Result<(), String> {
    verify_database(conn)?;
    let schema_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if schema_version != LATEST_DATABASE_SCHEMA_VERSION {
        return Err(format!(
            "The data library uses schema version {schema_version}, but the current version is {LATEST_DATABASE_SCHEMA_VERSION}."
        ));
    }
    let expected_tables = BTreeSet::from([
        "patients".to_string(),
        "sessions".to_string(),
        "session_inputs".to_string(),
        "evidence_ledger_cache".to_string(),
        "settings".to_string(),
        "note_formats".to_string(),
        "session_notes".to_string(),
        "note_revisions".to_string(),
        "patient_note_formats".to_string(),
        "recording_jobs".to_string(),
    ]);
    if table_names(conn)? != expected_tables {
        return Err("The data library does not match the current Gist table schema.".into());
    }
    for (table, columns) in [
        ("patients", &["id", "name", "created_at", "updated_at"][..]),
        (
            "sessions",
            &[
                "id",
                "patient_id",
                "date",
                "start_time",
                "title",
                "session_type",
                "updated_at",
                "created_at",
            ][..],
        ),
        (
            "session_inputs",
            &[
                "id",
                "session_id",
                "kind",
                "source",
                "title",
                "text",
                "audio_file",
                "duration_seconds",
                "transcription_model",
                "include_in_notes",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "evidence_ledger_cache",
            &[
                "source_id",
                "source_fingerprint",
                "model_identity",
                "pipeline_version",
                "payload_json",
                "retry_count",
                "updated_at",
            ][..],
        ),
        ("settings", &["key", "value"][..]),
        (
            "note_formats",
            &[
                "id",
                "name",
                "prompt",
                "is_builtin",
                "hidden",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "session_notes",
            &[
                "id",
                "session_id",
                "format",
                "note",
                "llm_model",
                "created_at",
                "updated_at",
                "finalized_at",
            ][..],
        ),
        (
            "note_revisions",
            &[
                "id",
                "note_id",
                "revision_number",
                "content",
                "llm_model",
                "created_at",
                "supersedes_revision_id",
                "amendment_reason",
            ][..],
        ),
        (
            "patient_note_formats",
            &["patient_id", "format_name", "position"][..],
        ),
        (
            "recording_jobs",
            &[
                "id",
                "session_id",
                "audio_file",
                "input_kind",
                "formats_json",
                "llm_model",
                "thinking",
                "diarize",
                "num_speakers",
                "created_session",
                "state",
                "error",
                "created_at",
                "updated_at",
            ][..],
        ),
    ] {
        let expected = columns
            .iter()
            .map(|column| (*column).to_string())
            .collect::<BTreeSet<_>>();
        if table_columns(conn, table)? != expected {
            return Err(format!(
                "The data library does not match the current {table} schema."
            ));
        }
    }
    for (table, expected) in [
        ("patients", &[][..]),
        ("sessions", &["patient_id:patients:id:CASCADE"][..]),
        ("session_inputs", &["session_id:sessions:id:CASCADE"][..]),
        (
            "evidence_ledger_cache",
            &["source_id:session_inputs:id:CASCADE"][..],
        ),
        ("settings", &[][..]),
        ("note_formats", &[][..]),
        ("session_notes", &["session_id:sessions:id:CASCADE"][..]),
        (
            "note_revisions",
            &[
                "note_id:session_notes:id:CASCADE",
                "supersedes_revision_id:note_revisions:id:NO ACTION",
            ][..],
        ),
        (
            "patient_note_formats",
            &["patient_id:patients:id:CASCADE"][..],
        ),
        ("recording_jobs", &["session_id:sessions:id:CASCADE"][..]),
    ] {
        let expected = expected
            .iter()
            .map(|key| (*key).to_string())
            .collect::<BTreeSet<_>>();
        if foreign_keys(conn, table)? != expected {
            return Err(format!(
                "The data library does not match the current {table} relationship schema."
            ));
        }
    }
    for (table, columns) in [
        ("patients", &["id"][..]),
        ("sessions", &["id"][..]),
        ("session_inputs", &["id"][..]),
        ("evidence_ledger_cache", &["source_id"][..]),
        ("settings", &["key"][..]),
        ("note_formats", &["id"][..]),
        ("note_formats", &["name"][..]),
        ("session_notes", &["id"][..]),
        ("session_notes", &["session_id", "format"][..]),
        ("note_revisions", &["id"][..]),
        ("note_revisions", &["note_id", "revision_number"][..]),
        ("patient_note_formats", &["patient_id", "format_name"][..]),
        ("recording_jobs", &["id"][..]),
    ] {
        if !has_unique_index(conn, table, columns)? {
            return Err(format!(
                "The data library is missing a required unique constraint on {table}."
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SnapshotPurpose {
    Portable,
    InternalRollback,
}

fn sanitize_snapshot(snapshot: &Connection, purpose: SnapshotPurpose) -> Result<(), String> {
    snapshot
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA secure_delete = ON;
             DELETE FROM evidence_ledger_cache;
             DELETE FROM recording_jobs;
             UPDATE session_inputs SET audio_file = NULL;",
        )
        .map_err(|e| e.to_string())?;
    if matches!(purpose, SnapshotPurpose::Portable) {
        snapshot
            .execute("DELETE FROM settings", [])
            .map_err(|e| e.to_string())?;
    }
    snapshot.execute_batch("VACUUM;").map_err(|e| e.to_string())
}

fn read_settings(conn: &Connection) -> Result<Vec<(String, String)>, String> {
    let mut statement = conn
        .prepare("SELECT key, value FROM settings ORDER BY key")
        .map_err(|e| e.to_string())?;
    let settings = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(settings)
}

fn install_settings(conn: &mut Connection, settings: &[(String, String)]) -> Result<(), String> {
    let transaction = conn.transaction().map_err(|e| e.to_string())?;
    transaction
        .execute("DELETE FROM settings", [])
        .map_err(|e| e.to_string())?;
    for (key, value) in settings {
        transaction
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

fn create_clean_snapshot(conn: &Connection, path: &Path) -> Result<BTreeMap<String, i64>, String> {
    conn.backup(DatabaseName::Main, path, None)
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    let snapshot = Connection::open(path).map_err(|e| e.to_string())?;
    verify_current_database(&snapshot)?;
    sanitize_snapshot(&snapshot, SnapshotPurpose::Portable)?;
    verify_current_database(&snapshot)?;
    record_counts(&snapshot)
}

fn create_rollback_snapshot(conn: &Connection, path: &Path) -> Result<(), String> {
    let result = (|| {
        conn.backup(DatabaseName::Main, path, None)
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        let snapshot = Connection::open(path).map_err(|e| e.to_string())?;
        verify_current_database(&snapshot)?;
        sanitize_snapshot(&snapshot, SnapshotPurpose::InternalRollback)?;
        verify_current_database(&snapshot)
    })();
    if result.is_err() {
        remove_file_if_present(path);
    }
    result
}

fn zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true)
        .unix_permissions(0o600)
}

fn atomic_zip_path(destination: &Path) -> Result<PathBuf, String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "The selected destination has no parent directory.".to_string())?;
    Ok(parent.join(format!(".gist-export-{}.partial", Uuid::new_v4())))
}

fn finish_atomic_export(partial: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(partial, destination).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if let Some(parent) = destination.parent() {
        if let Err(error) = File::open(parent).and_then(|directory| directory.sync_all()) {
            log::warn!(
                "Export was saved, but its parent directory could not be synchronized: {error}"
            );
        }
    }
    Ok(())
}

pub(crate) fn export_backup(
    conn: &Connection,
    destination: &Path,
    app_version: &str,
    workspace: &Path,
    passphrase: Option<&str>,
) -> Result<ExportResult, String> {
    validate_export_destination(destination, workspace)?;
    let passphrase = validate_export_passphrase(passphrase)?;
    let temp = private_temp_dir(workspace)?;
    let database_path = temp.path().join("library.sqlite3");
    let counts = create_clean_snapshot(conn, &database_path)?;
    let database_sha256 = sha256_file(&database_path)?;
    let manifest = BackupManifest {
        format: BACKUP_FORMAT.into(),
        format_version: BACKUP_FORMAT_VERSION,
        created_by_gist_version: app_version.into(),
        created_at: Utc::now().to_rfc3339(),
        database_schema_version: LATEST_DATABASE_SCHEMA_VERSION,
        database_file: "library.sqlite3".into(),
        database_sha256: database_sha256.clone(),
        contains_audio: false,
        contains_diagnostics: false,
        record_counts: counts.clone(),
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let checksum_text = format!("{database_sha256}  library.sqlite3\n");

    let zip_destination = passphrase
        .map(|_| temp.path().join("backup.gistbackup"))
        .unwrap_or_else(|| destination.to_path_buf());
    let partial = atomic_zip_path(&zip_destination)?;
    let result = (|| {
        let file = open_private_file(&partial)?;
        let mut zip = ZipWriter::new(file);
        zip.start_file("manifest.json", zip_options())
            .map_err(|e| e.to_string())?;
        zip.write_all(&manifest_json).map_err(|e| e.to_string())?;
        zip.start_file("library.sqlite3", zip_options())
            .map_err(|e| e.to_string())?;
        let mut database = File::open(&database_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut database, &mut zip).map_err(|e| e.to_string())?;
        zip.start_file("SHA256SUMS", zip_options())
            .map_err(|e| e.to_string())?;
        zip.write_all(checksum_text.as_bytes())
            .map_err(|e| e.to_string())?;
        zip.start_file("README.txt", zip_options())
            .map_err(|e| e.to_string())?;
        zip.write_all(b"Gist restorable backup\n\nThis archive contains sensitive clinical records, custom templates, and patient template preferences. It intentionally excludes audio, models, caches, logs, developer diagnostics, and application settings. Model names attached to existing notes are historical provenance, not model-selection settings. Restore it from Gist Settings.\n")
            .map_err(|e| e.to_string())?;
        let file = zip.finish().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        finish_atomic_export(&partial, &zip_destination)
    })();
    if result.is_err() {
        remove_file_if_present(&partial);
    }
    result?;
    if let Some(passphrase) = passphrase {
        let encrypted_partial = atomic_zip_path(destination)?;
        let encryption_result = encrypt_age(&zip_destination, &encrypted_partial, passphrase)
            .and_then(|()| finish_atomic_export(&encrypted_partial, destination));
        if encryption_result.is_err() {
            remove_file_if_present(&encrypted_partial);
        }
        encryption_result?;
    }

    Ok(ExportResult {
        path: destination.to_string_lossy().into_owned(),
        patient_count: *counts.get("patients").unwrap_or(&0),
        session_count: *counts.get("sessions").unwrap_or(&0),
    })
}

fn read_limited_entry(
    archive: &mut ZipArchive<File>,
    name: &str,
    maximum: u64,
) -> Result<Vec<u8>, String> {
    let entry = archive
        .by_name(name)
        .map_err(|_| format!("Missing {name}."))?;
    if entry.size() > maximum {
        return Err(format!("{name} exceeds the supported size limit."));
    }
    let mut contents = Vec::with_capacity(entry.size() as usize);
    entry
        .take(maximum + 1)
        .read_to_end(&mut contents)
        .map_err(|e| e.to_string())?;
    if contents.len() as u64 > maximum {
        return Err(format!("{name} exceeds the supported size limit."));
    }
    Ok(contents)
}

fn validate_zip_entries(archive: &mut ZipArchive<File>) -> Result<(), String> {
    let allowed = HashSet::from([
        "manifest.json",
        "library.sqlite3",
        "SHA256SUMS",
        "README.txt",
    ]);
    if archive.len() != allowed.len() {
        return Err("The backup must contain exactly four recognized files.".into());
    }
    let mut seen = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name();
        if !allowed.contains(name) || entry.is_dir() || entry.enclosed_name().is_none() {
            return Err(format!("Unexpected or unsafe backup entry: {name}"));
        }
        if !seen.insert(name.to_string()) {
            return Err(format!("Duplicate backup entry: {name}"));
        }
        let maximum = if name == "library.sqlite3" {
            MAX_DATABASE_BYTES
        } else {
            MAX_MANIFEST_BYTES
        };
        if entry.size() > maximum {
            return Err(format!(
                "Backup entry {name} exceeds the supported size limit."
            ));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(format!("Backup entry {name} is a symbolic link."));
        }
    }
    let missing = allowed
        .iter()
        .filter(|name| !seen.contains(**name))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "Backup is missing required entries: {}",
            missing.join(", ")
        ));
    }
    Ok(())
}

struct ReadableBackup {
    path: PathBuf,
    _temporary_directory: Option<TempDir>,
}

fn prepare_readable_backup(
    source: &Path,
    workspace: &Path,
    passphrase: Option<&str>,
) -> Result<ReadableBackup, String> {
    let metadata = std::fs::metadata(source).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("The selected backup is not a regular file.".into());
    }
    if metadata.len() > MAX_ENCRYPTED_EXPORT_BYTES {
        return Err("The selected backup exceeds the supported size limit.".into());
    }
    if !is_age_file(source)? {
        if metadata.len() > MAX_BACKUP_CONTAINER_BYTES {
            return Err("The selected backup exceeds the supported size limit.".into());
        }
        return Ok(ReadableBackup {
            path: source.to_path_buf(),
            _temporary_directory: None,
        });
    }

    let passphrase = supplied_passphrase(passphrase).ok_or_else(|| {
        "This backup is encrypted. Enter its passphrase before restoring it.".to_string()
    })?;
    let temporary_directory = private_temp_dir(workspace)?;
    let path = temporary_directory.path().join("decrypted.gistbackup");
    decrypt_age(source, &path, passphrase)?;
    Ok(ReadableBackup {
        path,
        _temporary_directory: Some(temporary_directory),
    })
}

fn read_backup_manifest(archive: &mut ZipArchive<File>) -> Result<BackupManifest, String> {
    validate_zip_entries(archive)?;
    let manifest_bytes = read_limited_entry(archive, "manifest.json", MAX_MANIFEST_BYTES)?;
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| "The backup manifest is invalid.".to_string())?;
    if manifest.format != BACKUP_FORMAT || manifest.format_version != BACKUP_FORMAT_VERSION {
        return Err("This backup format is not supported by this version of Gist.".into());
    }
    if manifest.database_file != "library.sqlite3" {
        return Err("The backup manifest references an unsupported database file.".into());
    }
    if manifest.database_sha256.len() != 64
        || !manifest
            .database_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("The backup manifest contains an invalid database checksum.".into());
    }
    let expected_count_names = RECORD_COUNT_NAMES.into_iter().collect::<BTreeSet<_>>();
    let actual_count_names = manifest
        .record_counts
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if actual_count_names != expected_count_names
        || manifest.record_counts.values().any(|count| *count < 0)
    {
        return Err("The backup manifest contains invalid record counts.".into());
    }
    if manifest.database_schema_version > LATEST_DATABASE_SCHEMA_VERSION {
        return Err(format!(
            "This backup requires a newer Gist data schema ({}).",
            manifest.database_schema_version
        ));
    }
    if manifest.contains_audio || manifest.contains_diagnostics {
        return Err("This backup declares unsupported sensitive attachments.".into());
    }
    let checksum_bytes = read_limited_entry(archive, "SHA256SUMS", MAX_MANIFEST_BYTES)?;
    let expected_checksum = format!("{}  library.sqlite3\n", manifest.database_sha256);
    if checksum_bytes != expected_checksum.as_bytes() {
        return Err("The backup checksum file does not match its manifest.".into());
    }
    Ok(manifest)
}

fn extract_database(
    archive: &mut ZipArchive<File>,
    destination: &Path,
    expected_sha256: &str,
) -> Result<(), String> {
    let mut entry = archive
        .by_name("library.sqlite3")
        .map_err(|_| "Missing library.sqlite3.".to_string())?;
    if entry.size() > MAX_DATABASE_BYTES {
        return Err("The backup database exceeds the supported size limit.".into());
    }
    let mut output = open_private_file(destination)?;
    let mut hasher = Sha256::new();
    let mut written = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = entry.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        written = written.saturating_add(count as u64);
        if written > MAX_DATABASE_BYTES {
            return Err("The backup database exceeds the supported size limit.".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        hasher.update(&buffer[..count]);
    }
    output.sync_all().map_err(|e| e.to_string())?;
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected_sha256 {
        return Err(
            "The backup checksum does not match. The file may be damaged or altered.".into(),
        );
    }
    Ok(())
}

fn rollback_directory(workspace: &Path) -> Result<PathBuf, String> {
    let directory = workspace.join("restore-rollbacks");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    Ok(directory)
}

fn prune_old_rollbacks(directory: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("pre-restore-") && name.ends_with(".sqlite3"))
        })
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| {
        std::cmp::Reverse(
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok(),
        )
    });
    for path in paths.into_iter().skip(keep) {
        if let Err(error) = std::fs::remove_file(&path) {
            log::warn!(
                "Could not prune old restore rollback {}: {error}",
                path.display()
            );
        }
    }
}

fn rollback_after_restore_failure(
    conn: &mut Connection,
    rollback_path: &Path,
    rollback_dir: &Path,
    failure: String,
) -> String {
    match conn.restore(
        DatabaseName::Main,
        rollback_path,
        None::<fn(rusqlite::backup::Progress)>,
    ) {
        Ok(()) => {
            let rollback_check = conn
                .execute_batch(
                    "PRAGMA foreign_keys = ON;
                     PRAGMA busy_timeout = 5000;
                     PRAGMA secure_delete = ON;
                     PRAGMA trusted_schema = OFF;",
                )
                .map_err(|error| error.to_string())
                .and_then(|()| verify_current_database(conn));
            if let Err(rollback_error) = rollback_check {
                return format!(
                    "{failure} The previous library was copied back but failed validation ({rollback_error}). The rollback copy remains at {}.",
                    rollback_path.display()
                );
            }
            prune_old_rollbacks(rollback_dir, 3);
            format!("{failure} The previous library was put back and verified.")
        }
        Err(rollback_error) => format!(
            "{failure} Automatic rollback also failed ({rollback_error}). The rollback copy remains at {}.",
            rollback_path.display()
        ),
    }
}

pub(crate) fn restore_backup(
    conn: &mut Connection,
    source: &Path,
    workspace: &Path,
    passphrase: Option<&str>,
) -> Result<RestoreResult, String> {
    // Application settings belong to the destination installation, not to the
    // portable written library. Keep them across activation while stripping
    // settings embedded by older backup writers.
    let local_settings = read_settings(conn)?;
    let readable = prepare_readable_backup(source, workspace, passphrase)?;
    let file = File::open(&readable.path).map_err(|e| e.to_string())?;
    let mut archive =
        ZipArchive::new(file).map_err(|_| "This is not a valid Gist backup.".to_string())?;
    let manifest = read_backup_manifest(&mut archive)?;

    let temp = private_temp_dir(workspace)?;
    let staged_path = temp.path().join("staged.sqlite3");
    extract_database(&mut archive, &staged_path, &manifest.database_sha256)?;

    {
        let mut staged = Connection::open(&staged_path).map_err(|e| e.to_string())?;
        staged
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| e.to_string())?;
        verify_database(&staged)?;
        let staged_schema_version: i64 = staged
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if staged_schema_version != manifest.database_schema_version {
            return Err("The backup schema version does not match its manifest.".into());
        }
        let backup_counts = record_counts(&staged)?;
        if backup_counts != manifest.record_counts {
            return Err("The backup record counts do not match its manifest.".into());
        }

        migrate_database(&mut staged)?;
        sanitize_snapshot(&staged, SnapshotPurpose::Portable)?;
        install_settings(&mut staged, &local_settings)?;
        verify_current_database(&staged)?;
    }

    let rollback_dir = rollback_directory(workspace)?;
    let rollback_path = rollback_dir.join(format!(
        "pre-restore-{}-{}.sqlite3",
        Local::now().format("%Y%m%d-%H%M%S"),
        Uuid::new_v4()
    ));
    create_rollback_snapshot(conn, &rollback_path)?;

    if let Err(error) = conn.restore(
        DatabaseName::Main,
        &staged_path,
        None::<fn(rusqlite::backup::Progress)>,
    ) {
        return Err(rollback_after_restore_failure(
            conn,
            &rollback_path,
            &rollback_dir,
            format!("Restore failed: {error}."),
        ));
    }
    let activation_check = (|| {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA secure_delete = ON;
             PRAGMA trusted_schema = OFF;",
        )
        .map_err(|e| e.to_string())?;
        verify_current_database(conn)
    })();
    if let Err(error) = activation_check {
        return Err(rollback_after_restore_failure(
            conn,
            &rollback_path,
            &rollback_dir,
            format!("The restored library failed validation: {error}."),
        ));
    }
    prune_old_rollbacks(&rollback_dir, 3);

    let counts = record_counts(conn)?;
    Ok(RestoreResult {
        patient_count: *counts.get("patients").unwrap_or(&0),
        session_count: *counts.get("sessions").unwrap_or(&0),
    })
}

fn safe_component(value: &str, fallback: &str) -> String {
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // Keep each ZIP path component comfortably below common 255-byte
    // filesystem limits even when labels contain four-byte Unicode scalars.
    let limited = trimmed
        .chars()
        .take(MAX_ARCHIVE_LABEL_CHARS)
        .collect::<String>();
    let component = if limited.is_empty() {
        fallback.to_string()
    } else {
        limited
    };
    let uppercase = component.to_ascii_uppercase();
    let reserved = matches!(uppercase.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (uppercase.len() == 4
            && (uppercase.starts_with("COM") || uppercase.starts_with("LPT"))
            && matches!(uppercase.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        format!("{component}_")
    } else {
        component
    }
}

fn unique_component(value: &str, fallback: &str, used: &mut HashSet<String>) -> String {
    let base = safe_component(value, fallback);
    if used.insert(base.to_lowercase()) {
        return base;
    }
    for number in 2_u64.. {
        let suffix = format!(" ({number})");
        let prefix_length = MAX_ARCHIVE_LABEL_CHARS.saturating_sub(suffix.chars().count());
        let prefix = base.chars().take(prefix_length).collect::<String>();
        let candidate = format!("{prefix}{suffix}");
        if used.insert(candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!("an unbounded numeric suffix always provides a unique archive name")
}

fn text_document(title: &str, fields: &[(&str, String)], body: Option<&str>) -> String {
    let mut document = format!(
        "{title}\n{}\n\n",
        "=".repeat(title.chars().count().clamp(3, 72))
    );
    for (label, value) in fields {
        document.push_str(label);
        document.push_str(": ");
        document.push_str(value);
        document.push('\n');
    }
    if !fields.is_empty() && body.is_some() {
        document.push('\n');
    }
    if let Some(body) = body {
        document.push_str(body);
        if !body.ends_with('\n') {
            document.push('\n');
        }
    }
    document
}

struct HumanArchiveWriter {
    zip: ZipWriter<File>,
    paths: HashSet<String>,
}

impl HumanArchiveWriter {
    fn new(file: File) -> Self {
        Self {
            zip: ZipWriter::new(file),
            paths: HashSet::new(),
        }
    }

    fn write_text(&mut self, path: impl Into<String>, text: String) -> Result<(), String> {
        let path = path.into();
        if !path.ends_with(".txt") {
            return Err("Human-readable archives may contain only .txt documents.".into());
        }
        if !self.paths.insert(path.to_lowercase()) {
            return Err(format!("The archive contains a duplicate path: {path}"));
        }
        let contents = text.as_bytes();
        self.zip
            .start_file(&path, zip_options())
            .map_err(|e| e.to_string())?;
        self.zip.write_all(contents).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        let file = self.zip.finish().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    }
}

fn write_archive_contents(
    conn: &Connection,
    app_version: &str,
    archive: &mut HumanArchiveWriter,
) -> Result<(i64, i64), String> {
    let mut patient_count = 0_i64;
    let mut session_count = 0_i64;
    let mut patient_names = HashSet::new();

    let mut patient_stmt = conn
        .prepare("SELECT id, name, created_at FROM patients ORDER BY name COLLATE NOCASE, id")
        .map_err(|e| e.to_string())?;
    let patients = patient_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    for (patient_id, patient_name, patient_created_at) in patients {
        patient_count += 1;
        let patient_folder = unique_component(&patient_name, "Patient", &mut patient_names);
        let patient_dir = format!("Patients/{patient_folder}");
        let mut preference_stmt = conn
            .prepare(
                "SELECT format_name FROM patient_note_formats WHERE patient_id = ?1 ORDER BY position",
            )
            .map_err(|e| e.to_string())?;
        let preferred_formats = preference_stmt
            .query_map(params![patient_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        archive.write_text(
            format!("{patient_dir}/Patient Information.txt"),
            text_document(
                "Patient Information",
                &[
                    ("Name", patient_name.clone()),
                    ("Record created", patient_created_at),
                    (
                        "Preferred note types",
                        if preferred_formats.is_empty() {
                            "Not configured".into()
                        } else {
                            preferred_formats.join(", ")
                        },
                    ),
                ],
                None,
            ),
        )?;

        let mut session_stmt = conn
            .prepare(
                "SELECT id, date, start_time, title, session_type
                 FROM sessions WHERE patient_id = ?1
                 ORDER BY date, COALESCE(start_time, ''), created_at, id",
            )
            .map_err(|e| e.to_string())?;
        let sessions = session_stmt
            .query_map(params![patient_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        let mut session_names = HashSet::new();
        for (session_id, date, start_time, title, session_type) in sessions {
            session_count += 1;
            let label = title.as_deref().unwrap_or("Session");
            let session_folder =
                unique_component(&format!("{date} - {label}"), "Session", &mut session_names);
            let session_dir = format!("{patient_dir}/Sessions/{session_folder}");
            archive.write_text(
                format!("{session_dir}/Session Information.txt"),
                text_document(
                    "Session Information",
                    &[
                        ("Patient", patient_name.clone()),
                        ("Date", date),
                        (
                            "Start time",
                            start_time.unwrap_or_else(|| "Not recorded".into()),
                        ),
                        ("Title", title.unwrap_or_else(|| "Session".into())),
                        (
                            "Type",
                            session_type.unwrap_or_else(|| "Not recorded".into()),
                        ),
                    ],
                    None,
                ),
            )?;

            let mut input_stmt = conn
                .prepare(
                    "SELECT title, text
                     FROM session_inputs WHERE session_id = ?1 ORDER BY created_at, id",
                )
                .map_err(|e| e.to_string())?;
            let inputs = input_stmt
                .query_map(params![session_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let mut input_names = HashSet::new();
            for (input_title, text) in inputs {
                let file_name = format!(
                    "{}.txt",
                    unique_component(&input_title, "Session Material", &mut input_names)
                );
                archive.write_text(
                    format!("{session_dir}/Session Materials/{file_name}"),
                    text_document(&input_title, &[], Some(&text)),
                )?;
            }

            let mut note_stmt = conn
                .prepare(
                    "SELECT id, format, note, created_at, updated_at, finalized_at
                     FROM session_notes WHERE session_id = ?1 ORDER BY format, id",
                )
                .map_err(|e| e.to_string())?;
            let notes = note_stmt
                .query_map(params![session_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let mut note_names = HashSet::new();
            for (note_id, format_name, note, note_created, note_updated, finalized_at) in notes {
                let format_file = unique_component(&format_name, "Note", &mut note_names);
                archive.write_text(
                    format!("{session_dir}/Notes/{format_file} Note.txt"),
                    text_document(
                        &format!("{format_name} Note"),
                        &[
                            (
                                "Last updated",
                                note_updated.unwrap_or_else(|| note_created.clone()),
                            ),
                            (
                                "Status",
                                finalized_at
                                    .map(|date| format!("Finalized on {date}"))
                                    .unwrap_or_else(|| "Not finalized".into()),
                            ),
                        ],
                        Some(note.as_deref().unwrap_or("")),
                    ),
                )?;

                let mut revision_stmt = conn
                    .prepare(
                        "SELECT revision_number, content, created_at, amendment_reason
                         FROM note_revisions WHERE note_id = ?1 ORDER BY revision_number",
                    )
                    .map_err(|e| e.to_string())?;
                let revisions = revision_stmt
                    .query_map(params![note_id], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    })
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                for (revision_number, content, revision_created, reason) in revisions {
                    let mut fields = vec![
                        ("Version", revision_number.to_string()),
                        ("Saved", revision_created),
                    ];
                    if let Some(reason) = reason {
                        fields.push(("Reason for change", reason));
                    }
                    archive.write_text(
                        format!(
                            "{session_dir}/Notes/Note History/{format_file}/Version {revision_number}.txt"
                        ),
                        text_document(
                            &format!("{format_name} Note — Version {revision_number}"),
                            &fields,
                            Some(&content),
                        ),
                    )?;
                }
            }
        }
    }

    let mut template_stmt = conn
        .prepare("SELECT name, prompt FROM note_formats WHERE is_builtin = 0 ORDER BY name, id")
        .map_err(|e| e.to_string())?;
    let templates = template_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut template_names = HashSet::new();
    for (name, prompt) in templates {
        let template_name = unique_component(&name, "Template", &mut template_names);
        archive.write_text(
            format!("Templates/{template_name}.txt"),
            text_document("Custom Note Template", &[("Name", name)], Some(&prompt)),
        )?;
    }

    archive.write_text(
        "Start Here.txt",
        text_document(
            "Gist Record Archive",
            &[
                (
                    "Created",
                    Local::now().format("%Y-%m-%d %H:%M %Z").to_string(),
                ),
                ("Gist version", app_version.to_string()),
                ("Patients", patient_count.to_string()),
                ("Sessions", session_count.to_string()),
            ],
            Some(
                "HOW TO USE THIS ARCHIVE\n\nOpen the Patients folder, choose a patient, and then open Sessions. Each session contains a short information file, the source texts used for that session, the current notes, and the saved note history. Custom note templates are in the Templates folder. Every document is an ordinary text file that can be opened with TextEdit, Microsoft Word, or almost any other writing application.\n\nIMPORTANT\n\nThis archive contains sensitive clinical records. Store it only in an appropriately secured location. It does not contain audio, application settings, downloaded models, caches, logs, or developer diagnostics. This archive is intended for reading and long-term storage; it cannot be restored into Gist. Use a Gist backup when you need to move the working library to another Mac.",
            ),
        ),
    )?;
    Ok((patient_count, session_count))
}

pub(crate) fn export_human_archive(
    conn: &Connection,
    destination: &Path,
    app_version: &str,
    workspace: &Path,
    passphrase: Option<&str>,
) -> Result<ExportResult, String> {
    validate_export_destination(destination, workspace)?;
    let passphrase = validate_export_passphrase(passphrase)?;
    let temp = private_temp_dir(workspace)?;
    let zip_destination = passphrase
        .map(|_| temp.path().join("records.zip"))
        .unwrap_or_else(|| destination.to_path_buf());
    let partial = atomic_zip_path(&zip_destination)?;
    let result = (|| {
        let file = open_private_file(&partial)?;
        let mut archive = HumanArchiveWriter::new(file);
        let counts = write_archive_contents(conn, app_version, &mut archive)?;
        archive.finish()?;
        finish_atomic_export(&partial, &zip_destination)?;
        Ok::<_, String>(counts)
    })();
    if result.is_err() {
        remove_file_if_present(&partial);
    }
    let (patient_count, session_count) = result?;
    if let Some(passphrase) = passphrase {
        let encrypted_partial = atomic_zip_path(destination)?;
        let encryption_result = encrypt_age(&zip_destination, &encrypted_partial, passphrase)
            .and_then(|()| finish_atomic_export(&encrypted_partial, destination));
        if encryption_result.is_err() {
            remove_file_if_present(&encrypted_partial);
        }
        encryption_result?;
    }
    Ok(ExportResult {
        path: destination.to_string_lossy().into_owned(),
        patient_count,
        session_count,
    })
}

pub(crate) fn inspect_backup(
    source: &Path,
    workspace: &Path,
    passphrase: Option<&str>,
) -> Result<ExportResult, String> {
    let readable = prepare_readable_backup(source, workspace, passphrase)?;
    let file = File::open(readable.path).map_err(|e| e.to_string())?;
    let mut archive =
        ZipArchive::new(file).map_err(|_| "This is not a valid Gist backup.".to_string())?;
    let manifest = read_backup_manifest(&mut archive)?;
    Ok(ExportResult {
        path: source.to_string_lossy().into_owned(),
        patient_count: *manifest.record_counts.get("patients").unwrap_or(&0),
        session_count: *manifest.record_counts.get("sessions").unwrap_or(&0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn assert_private_file(path: &Path) {
        let mode = std::fs::metadata(path)
            .expect("private file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    fn read_zip_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
        let file = File::open(path).expect("open ZIP fixture");
        let mut archive = ZipArchive::new(file).expect("read ZIP fixture");
        let mut entries = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("ZIP fixture entry");
            let mut contents = Vec::new();
            entry
                .read_to_end(&mut contents)
                .expect("read ZIP fixture entry");
            entries.push((entry.name().to_string(), contents));
        }
        entries
    }

    fn write_zip_entries(path: &Path, entries: &[(String, Vec<u8>)]) {
        let file = open_private_file(path).expect("create ZIP fixture");
        let mut archive = ZipWriter::new(file);
        for (name, contents) in entries {
            archive
                .start_file(name, zip_options())
                .expect("start ZIP fixture entry");
            archive
                .write_all(contents)
                .expect("write ZIP fixture entry");
        }
        archive.finish().expect("finish ZIP fixture");
    }

    fn test_directories(root: &TempDir) -> (PathBuf, PathBuf) {
        let workspace = root.path().join("app-data");
        let export_directory = root.path().join("exports");
        std::fs::create_dir(&workspace).expect("app-data directory");
        std::fs::create_dir(&export_directory).expect("export directory");
        (workspace, export_directory)
    }

    fn create_empty_backup(root: &TempDir) -> (PathBuf, PathBuf) {
        let (workspace, export_directory) = test_directories(root);
        let database_path = workspace.join("source.sqlite3");
        let mut database = Connection::open(database_path).expect("source database");
        migrate_database(&mut database).expect("source schema");
        let backup_path = export_directory.join("valid.gistbackup");
        export_backup(&database, &backup_path, "test", &workspace, None).expect("valid backup");
        (workspace, backup_path)
    }

    fn package_database(database: &Connection, path: &Path, schema_version: i64) {
        let database_path = database.path().expect("database path");
        let database_bytes = std::fs::read(database_path).expect("database bytes");
        let database_sha256 = sha256_file(Path::new(database_path)).expect("database checksum");
        let manifest = BackupManifest {
            format: BACKUP_FORMAT.into(),
            format_version: BACKUP_FORMAT_VERSION,
            created_by_gist_version: "test".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            database_schema_version: schema_version,
            database_file: "library.sqlite3".into(),
            database_sha256: database_sha256.clone(),
            contains_audio: false,
            contains_diagnostics: false,
            record_counts: record_counts(database).expect("record counts"),
        };
        write_zip_entries(
            path,
            &[
                (
                    "manifest.json".into(),
                    serde_json::to_vec_pretty(&manifest).expect("manifest"),
                ),
                ("library.sqlite3".into(), database_bytes),
                (
                    "SHA256SUMS".into(),
                    format!("{database_sha256}  library.sqlite3\n").into_bytes(),
                ),
                ("README.txt".into(), b"Synthetic test backup\n".to_vec()),
            ],
        );
    }

    #[test]
    fn sanitizes_archive_components() {
        assert_eq!(safe_component("  Jane / Doe: ", "Patient"), "Jane _ Doe_");
        assert_eq!(safe_component("///", "Patient"), "___");
        assert_eq!(safe_component("CON", "Patient"), "CON_");
        assert_eq!(safe_component("com1", "Patient"), "com1_");
        let mut used = HashSet::new();
        assert_eq!(
            unique_component("Jane Doe", "Patient", &mut used),
            "Jane Doe"
        );
        assert_eq!(
            unique_component("jane doe", "Patient", &mut used),
            "jane doe (2)"
        );
        assert_eq!(
            text_document("Transcript", &[], Some("Clinical text")),
            "Transcript\n==========\n\nClinical text\n"
        );
    }

    #[test]
    fn rejects_malformed_and_unsafe_backup_containers() {
        let root = TempDir::new().expect("workspace");
        let (workspace, valid_backup) = create_empty_backup(&root);
        let valid_entries = read_zip_entries(&valid_backup);
        let export_directory = valid_backup.parent().expect("export directory");

        let mut bad_checksum = valid_entries.clone();
        bad_checksum
            .iter_mut()
            .find(|(name, _)| name == "SHA256SUMS")
            .expect("checksum entry")
            .1 = b"not the manifest checksum\n".to_vec();
        let bad_checksum_path = export_directory.join("bad-checksum.gistbackup");
        write_zip_entries(&bad_checksum_path, &bad_checksum);
        assert!(inspect_backup(&bad_checksum_path, &workspace, None).is_err());

        let mut unsafe_path = valid_entries.clone();
        unsafe_path
            .iter_mut()
            .find(|(name, _)| name == "README.txt")
            .expect("README entry")
            .0 = "../README.txt".into();
        let unsafe_path_backup = export_directory.join("unsafe-path.gistbackup");
        write_zip_entries(&unsafe_path_backup, &unsafe_path);
        assert!(inspect_backup(&unsafe_path_backup, &workspace, None).is_err());

        let mut unknown_manifest_field = valid_entries.clone();
        let manifest_entry = unknown_manifest_field
            .iter_mut()
            .find(|(name, _)| name == "manifest.json")
            .expect("manifest entry");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&manifest_entry.1).expect("manifest JSON");
        manifest["unexpected_future_field"] = serde_json::json!(true);
        manifest_entry.1 = serde_json::to_vec_pretty(&manifest).expect("future manifest");
        let future_manifest_path = export_directory.join("future-manifest.gistbackup");
        write_zip_entries(&future_manifest_path, &unknown_manifest_field);
        assert!(inspect_backup(&future_manifest_path, &workspace, None).is_err());

        let mut newer_schema = valid_entries;
        let manifest_entry = newer_schema
            .iter_mut()
            .find(|(name, _)| name == "manifest.json")
            .expect("manifest entry");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&manifest_entry.1).expect("manifest JSON");
        manifest["database_schema_version"] = serde_json::json!(LATEST_DATABASE_SCHEMA_VERSION + 1);
        manifest_entry.1 = serde_json::to_vec_pretty(&manifest).expect("newer manifest");
        let newer_schema_path = export_directory.join("newer-schema.gistbackup");
        write_zip_entries(&newer_schema_path, &newer_schema);
        assert!(inspect_backup(&newer_schema_path, &workspace, None).is_err());

        let oversized_path = export_directory.join("oversized.gistbackup");
        let oversized = open_private_file(&oversized_path).expect("oversized fixture");
        oversized
            .set_len(MAX_BACKUP_CONTAINER_BYTES + 1)
            .expect("sparse oversized fixture");
        assert!(inspect_backup(&oversized_path, &workspace, None).is_err());
    }

    #[test]
    fn rejects_corrupted_payload_without_touching_current_library() {
        let root = TempDir::new().expect("workspace");
        let (workspace, valid_backup) = create_empty_backup(&root);
        let mut entries = read_zip_entries(&valid_backup);
        let database = &mut entries
            .iter_mut()
            .find(|(name, _)| name == "library.sqlite3")
            .expect("database entry")
            .1;
        database[0] ^= 0xff;
        let corrupted = valid_backup
            .parent()
            .expect("export directory")
            .join("corrupted.gistbackup");
        write_zip_entries(&corrupted, &entries);

        let current_path = workspace.join("current.sqlite3");
        let mut current = Connection::open(current_path).expect("current database");
        migrate_database(&mut current).expect("current schema");
        current
            .execute(
                "INSERT INTO patients (id, name, created_at, updated_at)
                 VALUES (?1, 'Current Patient', ?2, ?2)",
                params![Uuid::new_v4().to_string(), "2026-01-01T00:00:00Z"],
            )
            .expect("current patient");
        assert!(restore_backup(&mut current, &corrupted, &workspace, None).is_err());
        assert_eq!(
            current
                .query_row("SELECT name FROM patients", [], |row| row
                    .get::<_, String>(0))
                .expect("current patient remains"),
            "Current Patient"
        );
    }

    #[test]
    fn migrates_an_older_schema_backup_before_activation() {
        let root = TempDir::new().expect("workspace");
        let (workspace, export_directory) = test_directories(&root);
        let old_path = workspace.join("old.sqlite3");
        let mut old = Connection::open(&old_path).expect("old database");
        migrate_database(&mut old).expect("current schema baseline");
        let patient_id = Uuid::new_v4().to_string();
        old.execute(
            "INSERT INTO patients (id, name, created_at, updated_at)
             VALUES (?1, 'Historical Patient', ?2, ?2)",
            params![patient_id, "2026-01-01T00:00:00Z"],
        )
        .expect("historical patient");
        old.execute_batch(
            "DROP INDEX note_revisions_note_number;
             PRAGMA user_version = 2;",
        )
        .expect("older schema marker");
        let old_backup = export_directory.join("schema-v2.gistbackup");
        package_database(&old, &old_backup, 2);

        let restored_path = workspace.join("restored.sqlite3");
        let mut restored = Connection::open(restored_path).expect("restored database");
        migrate_database(&mut restored).expect("target schema");
        restore_backup(&mut restored, &old_backup, &workspace, None).expect("restore old backup");
        assert_eq!(
            restored
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .expect("schema version"),
            LATEST_DATABASE_SCHEMA_VERSION
        );
        assert_eq!(
            restored
                .query_row("SELECT name FROM patients", [], |row| row
                    .get::<_, String>(0))
                .expect("historical patient"),
            "Historical Patient"
        );
    }

    #[test]
    fn verified_rollback_restores_the_previous_library() {
        let root = TempDir::new().expect("workspace");
        let workspace = root.path().join("app-data");
        std::fs::create_dir(&workspace).expect("app-data directory");
        let database_path = workspace.join("current.sqlite3");
        let mut current = Connection::open(database_path).expect("current database");
        migrate_database(&mut current).expect("current schema");
        let patient_id = Uuid::new_v4().to_string();
        current
            .execute(
                "INSERT INTO patients (id, name, created_at, updated_at)
                 VALUES (?1, 'Before Restore', ?2, ?2)",
                params![patient_id, "2026-01-01T00:00:00Z"],
            )
            .expect("patient");
        let rollback_dir = rollback_directory(&workspace).expect("rollback directory");
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&rollback_dir)
                .expect("rollback directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let rollback_path = rollback_dir.join("pre-restore-test.sqlite3");
        create_rollback_snapshot(&current, &rollback_path).expect("rollback snapshot");
        current
            .execute("DELETE FROM patients", [])
            .expect("simulate failed activation");

        let message = rollback_after_restore_failure(
            &mut current,
            &rollback_path,
            &rollback_dir,
            "Activation failed.".into(),
        );
        assert!(message.contains("put back and verified"));
        assert_eq!(
            current
                .query_row("SELECT name FROM patients", [], |row| row
                    .get::<_, String>(0))
                .expect("rolled-back patient"),
            "Before Restore"
        );
    }

    #[test]
    fn rejects_unsafe_current_schema_objects_and_export_targets() {
        let root = TempDir::new().expect("workspace");
        let workspace = root.path().join("app-data");
        std::fs::create_dir(&workspace).expect("app-data directory");
        let stale = workspace.join(format!("{DATA_OPERATION_TEMP_PREFIX}stale"));
        let unrelated = workspace.join("unrelated-directory");
        std::fs::create_dir(&stale).expect("stale directory");
        std::fs::create_dir(&unrelated).expect("unrelated directory");
        std::fs::write(stale.join("clinical.tmp"), b"synthetic").expect("stale clinical fixture");
        cleanup_stale_data_operation_directories(&workspace).expect("stale cleanup");
        assert!(!stale.exists());
        assert!(unrelated.exists());
        let database_path = workspace.join("source.sqlite3");
        let mut database = Connection::open(database_path).expect("database");
        migrate_database(&mut database).expect("schema");
        assert!(export_backup(
            &database,
            &workspace.join("unsafe.gistbackup"),
            "test",
            &workspace,
            None,
        )
        .is_err());
        database
            .execute_batch(
                "CREATE TRIGGER unsafe_trigger AFTER INSERT ON settings
                 BEGIN DELETE FROM patients; END;",
            )
            .expect("unsafe trigger");
        assert!(verify_current_database(&database).is_err());
    }

    #[test]
    fn backup_round_trip_excludes_transient_data() {
        let root = TempDir::new().expect("workspace");
        let workspace = root.path().join("app-data");
        let export_directory = root.path().join("exports");
        std::fs::create_dir(&workspace).expect("app-data directory");
        std::fs::create_dir(&export_directory).expect("export directory");
        let patient_id = "00000000-0000-4000-8000-000000000001";
        let session_id = "00000000-0000-4000-8000-000000000002";
        let input_id = "00000000-0000-4000-8000-000000000003";
        let customized_template_id = "00000000-0000-4000-8000-000000000004";
        let custom_template_id = "00000000-0000-4000-8000-000000000005";
        let note_id = "00000000-0000-4000-8000-000000000006";
        let revision_id = "00000000-0000-4000-8000-000000000007";
        let recording_job_id = "00000000-0000-4000-8000-000000000008";
        let source_path = workspace.join("source.sqlite3");
        let mut source = Connection::open(&source_path).expect("source database");
        migrate_database(&mut source).expect("migrate source");
        source
            .execute(
                "INSERT INTO patients (id, name, created_at, updated_at) VALUES (?1, 'Example Patient', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![patient_id],
            )
            .expect("patient");
        source
            .execute(
                "INSERT INTO sessions (id, patient_id, date, created_at, updated_at) VALUES (?1, ?2, '2026-01-02', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z')",
                params![session_id, patient_id],
            )
            .expect("session");
        source
            .execute(
                "INSERT INTO session_inputs (id, session_id, kind, source, title, text, audio_file, include_in_notes, created_at, updated_at) VALUES (?1, ?2, 'session_transcript', 'recording', 'Transcript', 'Hello', '/tmp/recording.wav', 1, '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z')",
                params![input_id, session_id],
            )
            .expect("input");
        source
            .execute(
                "INSERT INTO evidence_ledger_cache (source_id, source_fingerprint, model_identity, pipeline_version, payload_json, updated_at) VALUES (?1, 'fingerprint', 'model', '1', '{}', '2026-01-02T00:00:00Z')",
                params![input_id],
            )
            .expect("cache");
        source
            .execute(
                "INSERT INTO recording_jobs (
                    id, session_id, audio_file, input_kind, formats_json,
                    llm_model, thinking, diarize, num_speakers,
                    created_session, state, created_at, updated_at
                 ) VALUES (
                    ?1, ?2, '/tmp/recording.wav', 'session_transcript',
                    '[\"Customized SOAP\"]', 'qwen-3.5-9b', 0, 1, 2, 0,
                    'recorded', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z'
                 )",
                params![recording_job_id, session_id],
            )
            .expect("recording recovery job");
        source
            .execute_batch(
                "INSERT INTO settings (key, value) VALUES
                    ('default_llm', 'qwen-3.5-9b'),
                    ('onboarding_completed', 'true'),
                    ('appearance', 'dark');",
            )
            .expect("source settings");
        source
            .execute(
                "INSERT INTO note_formats (id, name, prompt, is_builtin, hidden, created_at, updated_at)
                 VALUES (?1, 'Customized SOAP', 'Customized built-in prompt', 0, 0, '2026-01-01T00:00:00Z', '2026-01-02T00:00:00Z')",
                params![customized_template_id],
            )
            .expect("customized built-in template");
        source
            .execute(
                "INSERT INTO note_formats (id, name, prompt, is_builtin, hidden, created_at, updated_at)
                 VALUES (?1, 'Personal Narrative', 'Completely custom prompt', 0, 0, '2026-01-01T00:00:00Z', NULL)",
                params![custom_template_id],
            )
            .expect("custom template");
        source
            .execute(
                "INSERT INTO patient_note_formats (patient_id, format_name, position)
                 VALUES (?1, 'Customized SOAP', 0), (?1, 'Personal Narrative', 1)",
                params![patient_id],
            )
            .expect("patient template preferences");
        source
            .execute(
                "INSERT INTO session_notes (id, session_id, format, note, llm_model, created_at, updated_at)
                 VALUES (?1, ?2, 'Customized SOAP', 'Durable note', 'qwen-3.5-9b', '2026-01-02T01:00:00Z', '2026-01-02T01:00:00Z')",
                params![note_id, session_id],
            )
            .expect("note");
        source
            .execute(
                "INSERT INTO note_revisions (id, note_id, revision_number, content, llm_model, created_at)
                 VALUES (?1, ?2, 1, 'Durable note', 'qwen-3.5-9b', '2026-01-02T01:00:00Z')",
                params![revision_id, note_id],
            )
            .expect("note revision");

        let rollback_path = workspace.join("rollback.sqlite3");
        create_rollback_snapshot(&source, &rollback_path).expect("rollback snapshot");
        let rollback = Connection::open(&rollback_path).expect("rollback database");
        assert_eq!(
            read_settings(&rollback).expect("rollback settings"),
            vec![
                ("appearance".to_string(), "dark".to_string()),
                ("default_llm".to_string(), "qwen-3.5-9b".to_string()),
                ("onboarding_completed".to_string(), "true".to_string()),
            ]
        );
        assert_eq!(
            rollback
                .query_row("SELECT COUNT(*) FROM evidence_ledger_cache", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .expect("rollback cache count"),
            0
        );
        assert_eq!(
            rollback
                .query_row("SELECT COUNT(*) FROM recording_jobs", [], |row| row
                    .get::<_, i64>(0))
                .expect("rollback recovery count"),
            0
        );

        let backup_path = export_directory.join("round-trip.gistbackup");
        export_backup(&source, &backup_path, "test", &workspace, None).expect("export backup");
        #[cfg(unix)]
        assert_private_file(&backup_path);
        let inspected = inspect_backup(&backup_path, &workspace, None).expect("inspect backup");
        assert_eq!(inspected.patient_count, 1);
        assert_eq!(inspected.session_count, 1);

        let backup_file = File::open(&backup_path).expect("open backup");
        let mut backup_archive = ZipArchive::new(backup_file).expect("read backup");
        let backup_manifest = read_backup_manifest(&mut backup_archive).expect("backup manifest");
        let exported_database_path = workspace.join("exported.sqlite3");
        extract_database(
            &mut backup_archive,
            &exported_database_path,
            &backup_manifest.database_sha256,
        )
        .expect("extract exported database");
        let exported_database =
            Connection::open(&exported_database_path).expect("exported database");
        assert_eq!(
            exported_database
                .query_row("SELECT COUNT(*) FROM settings", [], |row| row
                    .get::<_, i64>(0))
                .expect("exported setting count"),
            0
        );

        let restored_path = workspace.join("restored.sqlite3");
        let mut restored = Connection::open(&restored_path).expect("restored database");
        migrate_database(&mut restored).expect("migrate restored");
        restored
            .execute_batch(
                "INSERT INTO settings (key, value) VALUES
                    ('default_llm', 'qwen-3.5-4b'),
                    ('appearance', 'system');",
            )
            .expect("destination settings");
        restore_backup(&mut restored, &backup_path, &workspace, None).expect("restore backup");
        assert_eq!(
            restored
                .query_row("SELECT COUNT(*) FROM patients", [], |row| row
                    .get::<_, i64>(0))
                .expect("patient count"),
            1
        );
        assert_eq!(
            restored
                .query_row(
                    "SELECT audio_file FROM session_inputs WHERE id = ?1",
                    params![input_id],
                    |row| row.get::<_, Option<String>>(0)
                )
                .expect("audio reference"),
            None
        );
        assert_eq!(
            restored
                .query_row("SELECT COUNT(*) FROM evidence_ledger_cache", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .expect("cache count"),
            0
        );
        assert_eq!(
            restored
                .query_row("SELECT COUNT(*) FROM recording_jobs", [], |row| row
                    .get::<_, i64>(0))
                .expect("recovery count"),
            0
        );
        assert_eq!(
            read_settings(&restored).expect("restored settings"),
            vec![
                ("appearance".to_string(), "system".to_string()),
                ("default_llm".to_string(), "qwen-3.5-4b".to_string()),
            ]
        );
        let restored_templates = restored
            .prepare("SELECT name, prompt FROM note_formats WHERE is_builtin = 0 ORDER BY name")
            .expect("template query")
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("template rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("templates");
        assert_eq!(
            restored_templates,
            vec![
                (
                    "Customized SOAP".to_string(),
                    "Customized built-in prompt".to_string(),
                ),
                (
                    "Personal Narrative".to_string(),
                    "Completely custom prompt".to_string(),
                ),
            ]
        );
        let restored_preferences = restored
            .prepare(
                "SELECT format_name FROM patient_note_formats WHERE patient_id = ?1 ORDER BY position",
            )
            .expect("preference query")
            .query_map(params![patient_id], |row| row.get::<_, String>(0))
            .expect("preference rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("preferences");
        assert_eq!(
            restored_preferences,
            vec![
                "Customized SOAP".to_string(),
                "Personal Narrative".to_string(),
            ]
        );
        assert_eq!(
            restored
                .query_row(
                    "SELECT llm_model FROM session_notes WHERE id = ?1",
                    params![note_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .expect("note model provenance")
                .as_deref(),
            Some("qwen-3.5-9b")
        );
        assert_eq!(
            restored
                .query_row(
                    "SELECT llm_model FROM note_revisions WHERE id = ?1",
                    params![revision_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .expect("revision model provenance")
                .as_deref(),
            Some("qwen-3.5-9b")
        );

        let archive_path = export_directory.join("records.zip");
        let archive_result =
            export_human_archive(&restored, &archive_path, "test", &workspace, None)
                .expect("human archive");
        #[cfg(unix)]
        assert_private_file(&archive_path);
        assert_eq!(archive_result.patient_count, 1);
        assert_eq!(archive_result.session_count, 1);
        let archive_file = File::open(&archive_path).expect("open human archive");
        let mut archive = ZipArchive::new(archive_file).expect("read human archive");
        let mut documents = BTreeMap::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("archive entry");
            let name = entry.name().to_string();
            let mut contents = String::new();
            entry
                .read_to_string(&mut contents)
                .expect("read archive document");
            documents.insert(name, contents);
        }
        assert!(documents.keys().all(|name| name.ends_with(".txt")));
        assert!(documents.contains_key("Start Here.txt"));
        assert!(documents.contains_key("Patients/Example Patient/Patient Information.txt"));
        let session_dir = "Patients/Example Patient/Sessions/2026-01-02 - Session";
        assert!(documents.contains_key(&format!("{session_dir}/Session Information.txt")));
        assert!(
            documents[&format!("{session_dir}/Session Materials/Transcript.txt")].contains("Hello")
        );
        assert!(
            documents[&format!("{session_dir}/Notes/Customized SOAP Note.txt")]
                .contains("Durable note")
        );
        assert!(documents.contains_key(&format!(
            "{session_dir}/Notes/Note History/Customized SOAP/Version 1.txt"
        )));
        assert!(documents["Templates/Customized SOAP.txt"].contains("Customized built-in prompt"));
        assert!(documents["Templates/Personal Narrative.txt"].contains("Completely custom prompt"));
        for (name, contents) in documents {
            assert!(!name.contains("00000000-0000"));
            assert!(!contents.contains("00000000-0000"));
            assert!(!contents.contains("qwen-3.5-9b"));
            assert!(!contents.contains("llm_model"));
        }

        let encrypted_path = export_directory.join("encrypted.gistbackup.age");
        export_backup(
            &restored,
            &encrypted_path,
            "test",
            &workspace,
            Some("correct horse battery staple"),
        )
        .expect("encrypted backup");
        #[cfg(unix)]
        assert_private_file(&encrypted_path);
        assert!(inspect_backup(&encrypted_path, &workspace, Some("incorrect passphrase")).is_err());
        assert_eq!(
            inspect_backup(
                &encrypted_path,
                &workspace,
                Some("correct horse battery staple")
            )
            .expect("inspect encrypted backup")
            .session_count,
            1
        );
        let encrypted_restored_path = workspace.join("encrypted-restored.sqlite3");
        let mut encrypted_restored =
            Connection::open(encrypted_restored_path).expect("encrypted restored database");
        migrate_database(&mut encrypted_restored).expect("encrypted target schema");
        restore_backup(
            &mut encrypted_restored,
            &encrypted_path,
            &workspace,
            Some("correct horse battery staple"),
        )
        .expect("restore encrypted backup");
        assert_eq!(
            encrypted_restored
                .query_row("SELECT COUNT(*) FROM patients", [], |row| row
                    .get::<_, i64>(0))
                .expect("encrypted restored patient count"),
            1
        );

        let spaced_path = export_directory.join("spaced.gistbackup.age");
        export_backup(
            &restored,
            &spaced_path,
            "test",
            &workspace,
            Some("  passphrases preserve spaces  "),
        )
        .expect("space-preserving encrypted backup");
        assert!(inspect_backup(
            &spaced_path,
            &workspace,
            Some("passphrases preserve spaces")
        )
        .is_err());
        inspect_backup(
            &spaced_path,
            &workspace,
            Some("  passphrases preserve spaces  "),
        )
        .expect("inspect space-preserving backup");

        // Restore remains compatible with older encrypted exports that used a
        // shorter passphrase, even though new exports enforce a stronger floor.
        let legacy_encrypted_path = export_directory.join("legacy-short.age");
        encrypt_age(&backup_path, &legacy_encrypted_path, "short")
            .expect("legacy encrypted backup");
        inspect_backup(&legacy_encrypted_path, &workspace, Some("short"))
            .expect("inspect legacy encrypted backup");
    }
}

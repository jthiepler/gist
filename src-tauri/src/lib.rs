use chrono::Local;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

mod audio;

// ── Database ──────────────────────────────────────────────────────────────

const DEFAULT_DIARIZATION_SPEAKERS: i64 = 2;
const MIN_DIARIZATION_SPEAKERS: i64 = 2;
const MAX_DIARIZATION_SPEAKERS: i64 = 4;

fn developer_features_available() -> bool {
    cfg!(debug_assertions) || option_env!("GIST_DEVELOPER_FEATURES") == Some("1")
}

const SESSION_COLUMNS: &str =
    "id, patient_id, date, start_time, title, session_type, updated_at, created_at";

fn map_session(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        date: row.get(2)?,
        start_time: row.get(3)?,
        title: row.get(4)?,
        session_type: row.get(5)?,
        updated_at: row.get(6)?,
        created_at: row.get(7)?,
        inputs: Vec::new(),
        notes: Vec::new(),
    })
}

fn fetch_session_inputs(conn: &Connection, session_id: &str) -> Result<Vec<SessionInput>, String> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, kind, source, title, text, audio_file, duration_seconds, transcription_model, include_in_notes, created_at, updated_at
         FROM session_inputs
         WHERE session_id = ?1
         ORDER BY created_at ASC",
    ).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(SessionInput {
                id: row.get(0)?,
                session_id: row.get(1)?,
                kind: row.get(2)?,
                source: row.get(3)?,
                title: row.get(4)?,
                text: row.get(5)?,
                audio_file: row.get(6)?,
                duration_seconds: row.get(7)?,
                transcription_model: row.get(8)?,
                include_in_notes: row.get::<_, i64>(9)? != 0,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn fetch_session_notes(conn: &Connection, session_id: &str) -> Result<Vec<SessionNote>, String> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, format, note, llm_model, created_at FROM session_notes WHERE session_id = ?1 ORDER BY format ASC",
    ).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(SessionNote {
                id: row.get(0)?,
                session_id: row.get(1)?,
                format: row.get(2)?,
                note: row.get(3)?,
                llm_model: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

struct Database {
    conn: Connection,
}

impl Database {
    fn new(app: &AppHandle) -> Result<Self, String> {
        let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
        let db_path = app_dir.join("gist.db");
        let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS patients (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                patient_id TEXT NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
                date TEXT NOT NULL,
                start_time TEXT,
                title TEXT,
                session_type TEXT,
                updated_at TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS session_inputs (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                source TEXT NOT NULL,
                title TEXT NOT NULL,
                text TEXT NOT NULL,
                audio_file TEXT,
                duration_seconds REAL,
                transcription_model TEXT,
                include_in_notes INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS evidence_ledger_cache (
                source_id TEXT PRIMARY KEY REFERENCES session_inputs(id) ON DELETE CASCADE,
                source_fingerprint TEXT NOT NULL,
                model_identity TEXT NOT NULL,
                pipeline_version TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                retry_count INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS note_formats (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                prompt TEXT NOT NULL,
                is_builtin INTEGER DEFAULT 0,
                hidden INTEGER DEFAULT 0,
                created_at TEXT NOT NULL
            );
             CREATE TABLE IF NOT EXISTS session_notes (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                format TEXT NOT NULL,
                note TEXT,
                llm_model TEXT,
                created_at TEXT NOT NULL,
                UNIQUE(session_id, format)
            );
             CREATE TABLE IF NOT EXISTS recording_jobs (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                audio_file TEXT NOT NULL,
                input_kind TEXT NOT NULL,
                formats_json TEXT NOT NULL,
                llm_model TEXT NOT NULL,
                thinking INTEGER NOT NULL,
                diarize INTEGER NOT NULL,
                num_speakers INTEGER NOT NULL DEFAULT 2,
                created_session INTEGER NOT NULL,
                state TEXT NOT NULL,
                error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .map_err(|e| e.to_string())?;

        let _ = conn.execute(
            "ALTER TABLE note_formats ADD COLUMN hidden INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN start_time TEXT", []);
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN title TEXT", []);
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN session_type TEXT", []);
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN updated_at TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE recording_jobs ADD COLUMN num_speakers INTEGER NOT NULL DEFAULT 2",
            [],
        );

        // Built-in templates contain format-specific instructions only. Keep them
        // synchronized with the bundled catalog; UI customization creates a
        // separate custom template rather than mutating a built-in.
        let now = Local::now().to_rfc3339();
        for (name, prompt) in default_formats() {
            let builtin_exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM note_formats WHERE name = ?1 AND is_builtin = 1",
                    params![name],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if builtin_exists == 0 {
                let id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO note_formats (id, name, prompt, is_builtin, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
                    params![id, name, prompt, now],
                )
                .map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE note_formats SET prompt = ?1 WHERE name = ?2 AND is_builtin = 1",
                    params![prompt, name],
                )
                .map_err(|e| e.to_string())?;
            }
        }

        Ok(Database { conn })
    }
}

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Patient {
    id: String,
    name: String,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Session {
    id: String,
    patient_id: String,
    date: String,
    start_time: Option<String>,
    title: Option<String>,
    session_type: Option<String>,
    updated_at: Option<String>,
    created_at: String,
    #[serde(default)]
    inputs: Vec<SessionInput>,
    #[serde(default)]
    notes: Vec<SessionNote>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SessionInput {
    id: String,
    session_id: String,
    kind: String,
    source: String,
    title: String,
    text: String,
    audio_file: Option<String>,
    duration_seconds: Option<f64>,
    transcription_model: Option<String>,
    include_in_notes: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SessionNote {
    id: String,
    session_id: String,
    format: String,
    note: Option<String>,
    llm_model: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct DiagnosticExportResult {
    path: String,
    run_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NoteFormatTemplate {
    id: String,
    name: String,
    prompt: String,
    is_builtin: bool,
    hidden: bool,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RecordingJob {
    id: String,
    session_id: String,
    audio_file: String,
    input_kind: String,
    formats: Vec<String>,
    llm_model: String,
    thinking: bool,
    diarize: bool,
    num_speakers: i64,
    created_session: bool,
    state: String,
    error: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct StartRecordingData {
    session_id: String,
    input_kind: String,
    formats: Vec<String>,
    llm_model: String,
    thinking: bool,
    diarize: bool,
    #[serde(default = "default_num_speakers")]
    num_speakers: i64,
    created_session: bool,
}

fn default_num_speakers() -> i64 {
    DEFAULT_DIARIZATION_SPEAKERS
}

fn validate_num_speakers(num_speakers: i64) -> Result<i64, String> {
    if (MIN_DIARIZATION_SPEAKERS..=MAX_DIARIZATION_SPEAKERS).contains(&num_speakers) {
        Ok(num_speakers)
    } else {
        Err(format!(
            "Number of speakers must be between {MIN_DIARIZATION_SPEAKERS} and {MAX_DIARIZATION_SPEAKERS}."
        ))
    }
}

#[derive(Debug, Deserialize)]
struct CreatePatient {
    name: String,
}

#[derive(Debug, Deserialize)]
struct UpdatePatient {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct CreateSession {
    patient_id: String,
    date: String,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    session_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateSession {
    id: String,
    date: String,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    session_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateSessionInput {
    session_id: String,
    kind: String,
    source: String,
    title: String,
    text: String,
    #[serde(default)]
    audio_file: Option<String>,
    #[serde(default)]
    duration_seconds: Option<f64>,
    #[serde(default)]
    transcription_model: Option<String>,
    #[serde(default = "default_include_in_notes")]
    include_in_notes: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateSessionInput {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    include_in_notes: Option<bool>,
}

fn default_include_in_notes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct CreateNoteFormat {
    name: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct UpdateNoteFormat {
    id: String,
    name: String,
    prompt: String,
}

// ── Default Format Prompts ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DefaultFormatSection {
    heading: String,
    guidance: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DefaultFormat {
    name: String,
    description: String,
    sections: Vec<DefaultFormatSection>,
}

#[derive(Debug, Deserialize)]
struct DefaultFormatCatalog {
    formats: Vec<DefaultFormat>,
}

fn default_formats() -> Vec<(String, String)> {
    let catalog: DefaultFormatCatalog =
        serde_json::from_str(include_str!("../../gist/formats/defaults.json"))
            .expect("bundled clinical note format defaults must be valid JSON");
    let DefaultFormatCatalog { formats } = catalog;
    formats
        .into_iter()
        .map(|format| {
            let mut prompt = format!(
                "{}. Generate a clinical note from the labeled source materials.\nThe application's mandatory system rules remain controlling. These format instructions control structure only and never authorize filling an evidentiary gap.\n\nRequired output format:\n\n",
                format.description
            );
            for section in format.sections {
                prompt.push_str("**");
                prompt.push_str(&section.heading);
                prompt.push_str(":**\n");
                for item in section.guidance {
                    prompt.push_str("- ");
                    prompt.push_str(&item);
                    prompt.push('\n');
                }
                prompt.push('\n');
            }
            (format.name, prompt.trim_end().to_string())
        })
        .collect()
}

// ── Sidecar ───────────────────────────────────────────────────────────────

struct SidecarState {
    request_tx: Option<mpsc::UnboundedSender<String>>,
    response_tx: Option<(String, oneshot::Sender<Result<Value, String>>)>,
    child: Option<Child>,
    sidecar_log: Option<Arc<Mutex<File>>>,
    generation: u64,
    started: bool,
    busy: bool,
}

type SharedSidecarState = Arc<Mutex<SidecarState>>;

const DEFAULT_RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const NOTE_GENERATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60 * 60);
const TRANSCRIPTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8 * 60 * 60);
const MODEL_DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
const SIDECAR_CANCEL_GRACE: std::time::Duration = std::time::Duration::from_secs(2);
const SIDECAR_CANCEL_POLL: std::time::Duration = std::time::Duration::from_millis(50);

fn rpc_timeout(operation_type: &str) -> std::time::Duration {
    match operation_type {
        "generate_note" | "generate_notes" => NOTE_GENERATION_TIMEOUT,
        "transcribe" => TRANSCRIPTION_TIMEOUT,
        "download_model" => MODEL_DOWNLOAD_TIMEOUT,
        _ => DEFAULT_RPC_TIMEOUT,
    }
}

fn cancel_message(request_id: &str) -> String {
    serde_json::json!({ "type": "cancel", "request_id": request_id }).to_string()
}

fn emit_sidecar_state(app: &AppHandle, busy: bool) {
    let _ = app.emit("sidecar-state", serde_json::json!({ "busy": busy }));
}

/// Append a privacy-safe lifecycle event from the Tauri side of the sidecar
/// bridge. The sidecar's stderr uses the same file, so this gives us one
/// chronological log for both processes without ever serializing RPC payloads.
fn log_sidecar_event(log_file: Option<&Arc<Mutex<File>>>, level: &str, event: impl AsRef<str>) {
    let Some(log_file) = log_file else {
        return;
    };
    let Ok(mut file) = log_file.lock() else {
        return;
    };
    let _ = writeln!(
        file,
        "{} {} [tauri] {}",
        Local::now().to_rfc3339(),
        level,
        event.as_ref()
    );
    let _ = file.flush();
}

// ── Sidecar Commands ──────────────────────────────────────────────────────

fn start_sidecar_process(app: &AppHandle, state: &SharedSidecarState) -> Result<String, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    if s.started {
        return Err("Sidecar already running".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let sidecar_path = resource_dir
        .join("resources")
        .join("gist-sidecar")
        .join("gist-sidecar");

    // Dev fallback: look for the sidecar in the project's dist directory
    let sidecar_path = if sidecar_path.exists() {
        sidecar_path
    } else {
        let dev_path = std::env::current_dir()
            .unwrap_or_default()
            .join("dist/gist-sidecar/gist-sidecar");
        if dev_path.exists() {
            dev_path
        } else {
            return Err(format!(
                "Sidecar not found. Looked at: {} and {}",
                sidecar_path.display(),
                dev_path.display()
            ));
        }
    };

    // MLX needs to find libmlx.dylib via DYLD_FALLBACK_LIBRARY_PATH
    let mlx_lib_dir = sidecar_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("_internal/mlx/lib");
    let dyld_path = std::env::var("DYLD_FALLBACK_LIBRARY_PATH").unwrap_or_default();
    let dyld_path = if dyld_path.is_empty() {
        mlx_lib_dir.to_string_lossy().into_owned()
    } else {
        format!("{}:{}", mlx_lib_dir.display(), dyld_path)
    };

    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
    let model_cache_dir = app_dir.join("models");
    std::fs::create_dir_all(&model_cache_dir).map_err(|e| e.to_string())?;
    let log_path = app_dir.join("sidecar.log");
    let mut log_options = OpenOptions::new();
    log_options.create(true).append(true);
    #[cfg(unix)]
    log_options.mode(0o600);
    let log_handle = log_options.open(&log_path).map_err(|e| {
        format!(
            "Failed to open sidecar log at {}: {}",
            log_path.display(),
            e
        )
    })?;
    let log_file = Arc::new(Mutex::new(log_handle));
    let stderr_file = log_file
        .lock()
        .map_err(|e| e.to_string())?
        .try_clone()
        .map_err(|e| format!("Failed to prepare sidecar log: {}", e))?;

    let mut sidecar_command = Command::new(&sidecar_path);
    sidecar_command
        .arg("serve")
        .env("DYLD_FALLBACK_LIBRARY_PATH", &dyld_path)
        .env("HF_HUB_CACHE", &model_cache_dir)
        .env("GIST_DATABASE_PATH", app_dir.join("gist.db"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file));
    if developer_features_available() {
        sidecar_command.env(
            "GIST_DIAGNOSTICS_DIR",
            app_dir.join("note-generation-diagnostics"),
        );
    }
    let mut child = sidecar_command
        .spawn()
        .map_err(|e| format!("Failed to start sidecar: {}", e))?;
    s.generation = s.generation.wrapping_add(1);
    let generation = s.generation;

    log_sidecar_event(
        Some(&log_file),
        "INFO",
        format!(
            "event=sidecar_started pid={} model_cache_configured=true",
            child.id()
        ),
    );

    let stdin = child.stdin.take().ok_or("Failed to capture stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;

    let (req_tx, req_rx) = mpsc::unbounded_channel::<String>();

    // Writer task: owns stdin, drains request channel
    let writer_log = log_file.clone();
    std::thread::spawn(move || {
        let mut stdin = stdin;
        let mut rx = req_rx;
        while let Some(line) = rx.blocking_recv() {
            if writeln!(stdin, "{}", line).is_err() {
                log_sidecar_event(
                    Some(&writer_log),
                    "ERROR",
                    "event=sidecar_stdin_write_failed",
                );
                break;
            }
            if stdin.flush().is_err() {
                log_sidecar_event(
                    Some(&writer_log),
                    "ERROR",
                    "event=sidecar_stdin_flush_failed",
                );
                break;
            }
        }
        // stdin dropped here → sidecar gets EOF on its stdin
        log_sidecar_event(Some(&writer_log), "INFO", "event=sidecar_stdin_closed");
    });

    // Reader task: owns stdout, emits progress, routes responses
    let app_clone = app.clone();
    let state_clone: SharedSidecarState = state.clone();
    let reader_log = log_file.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut last_progress_stage = String::new();
        let mut last_progress_percent = 0_u64;
        for line in reader.lines() {
            let current_generation = state_clone
                .lock()
                .map(|s| s.generation == generation)
                .unwrap_or(false);
            if !current_generation {
                break;
            }
            let line = match line {
                Ok(l) => l,
                Err(_) => {
                    log_sidecar_event(
                        Some(&reader_log),
                        "ERROR",
                        "event=sidecar_stdout_read_failed",
                    );
                    break;
                } // EOF or error → sidecar died
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let parsed: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => {
                    log_sidecar_event(
                        Some(&reader_log),
                        "WARN",
                        "event=sidecar_stdout_invalid_json",
                    );
                    continue;
                }
            };

            match parsed.get("type").and_then(|v| v.as_str()) {
                Some("progress") => {
                    let percent = parsed
                        .get("percent")
                        .and_then(Value::as_u64)
                        .unwrap_or_default();
                    let stage = parsed
                        .get("stage")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    if stage != last_progress_stage
                        || percent == 0
                        || percent == 100
                        || percent >= last_progress_percent.saturating_add(10)
                    {
                        log_sidecar_event(
                            Some(&reader_log),
                            "INFO",
                            format!("event=sidecar_progress percent={} stage={}", percent, stage),
                        );
                        last_progress_stage = stage.to_string();
                        last_progress_percent = percent;
                    }
                    let _ = app_clone.emit("sidecar-progress", &parsed);
                }
                Some("result") | Some("pong") => {
                    let request_id = parsed.get("request_id").and_then(|value| value.as_str());
                    log_sidecar_event(
                        Some(&reader_log),
                        "INFO",
                        format!(
                            "event=sidecar_response_received response_type={} request_id={}",
                            parsed
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown"),
                            request_id.unwrap_or("unknown")
                        ),
                    );
                    let resp_tx = {
                        if let Ok(mut s) = state_clone.lock() {
                            if s.response_tx.as_ref().map(|(id, _)| Some(id.as_str()))
                                == Some(request_id)
                            {
                                s.response_tx.take().map(|(_, tx)| tx)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    let matched = resp_tx.is_some();
                    if let Some(tx) = resp_tx {
                        let _ = tx.send(Ok(parsed));
                    }
                    if matched {
                        if let Ok(mut s) = state_clone.lock() {
                            s.busy = false;
                        }
                        emit_sidecar_state(&app_clone, false);
                    }
                }
                Some("error") => {
                    let msg = parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();
                    let request_id = parsed.get("request_id").and_then(|value| value.as_str());
                    log_sidecar_event(
                        Some(&reader_log),
                        "ERROR",
                        format!(
                            "event=sidecar_error_response request_id={} message_length={}",
                            request_id.unwrap_or("unknown"),
                            msg.len()
                        ),
                    );
                    let resp_tx = {
                        if let Ok(mut s) = state_clone.lock() {
                            if s.response_tx.as_ref().map(|(id, _)| Some(id.as_str()))
                                == Some(request_id)
                            {
                                s.response_tx.take().map(|(_, tx)| tx)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    let matched = resp_tx.is_some();
                    if let Some(tx) = resp_tx {
                        let _ = tx.send(Err(msg));
                    }
                    if matched {
                        if let Ok(mut s) = state_clone.lock() {
                            s.busy = false;
                        }
                        emit_sidecar_state(&app_clone, false);
                    }
                }
                _ => {}
            }
        }

        // EOF on stdout — sidecar died or was intentionally stopped.
        let is_current_generation = state_clone
            .lock()
            .map(|s| s.generation == generation)
            .unwrap_or(false);
        if !is_current_generation {
            return;
        }
        let unexpected_exit = state_clone.lock().map(|s| s.started).unwrap_or(false);
        log_sidecar_event(
            Some(&reader_log),
            if unexpected_exit { "ERROR" } else { "INFO" },
            "event=sidecar_stdout_closed",
        );
        if let Ok(mut s) = state_clone.lock() {
            if s.generation != generation {
                return;
            }
            s.started = false;
            s.busy = false;
            s.sidecar_log = None;
            if let Some((_, tx)) = s.response_tx.take() {
                let _ = tx.send(Err("Sidecar closed connection unexpectedly".into()));
            }
        }
        emit_sidecar_state(&app_clone, false);
    });

    s.request_tx = Some(req_tx);
    s.child = Some(child);
    s.sidecar_log = Some(log_file);
    s.started = true;
    s.busy = false;

    Ok("Sidecar started".into())
}

#[tauri::command]
async fn start_sidecar(
    app: AppHandle,
    state: State<'_, SharedSidecarState>,
) -> Result<String, String> {
    start_sidecar_process(&app, state.inner())
}

#[tauri::command]
async fn stop_sidecar(
    app: AppHandle,
    state: State<'_, SharedSidecarState>,
) -> Result<String, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    let log_file = s.sidecar_log.clone();
    log_sidecar_event(log_file.as_ref(), "INFO", "event=sidecar_stop_requested");
    s.started = false;
    s.busy = false;
    s.request_tx.take(); // drop → writer task exits, stdin closes

    if let Some((_, tx)) = s.response_tx.take() {
        let _ = tx.send(Err("Sidecar stopped".into()));
    }

    if let Some(mut child) = s.child.take() {
        // Try graceful kill, then force kill after 5 seconds
        let _ = child.kill();
        let _ = child.wait();
    }

    s.sidecar_log = None;

    emit_sidecar_state(&app, false);
    Ok("Sidecar stopped".into())
}

#[tauri::command]
async fn rpc_call(
    app: AppHandle,
    state: State<'_, SharedSidecarState>,
    request: String,
) -> Result<Value, String> {
    let (tx, mut rx) = oneshot::channel();
    let request_id = Uuid::new_v4().to_string();
    let mut request_value: Value = serde_json::from_str(&request)
        .map_err(|_| "Invalid request for processing engine".to_string())?;
    let operation_type = {
        let request_object = request_value
            .as_object_mut()
            .ok_or("Invalid request for processing engine")?;
        let operation_type = request_object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let capture_diagnostics = request_object
            .get("capture_diagnostics")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if capture_diagnostics && !developer_features_available() {
            return Err("Developer diagnostics are unavailable in release builds".into());
        }
        if !capture_diagnostics {
            request_object.remove("diagnostic_session_id");
        }
        request_object.insert("request_id".into(), Value::String(request_id.clone()));
        operation_type
    };
    let request = serde_json::to_string(&request_value).map_err(|e| e.to_string())?;
    // Prevent idle/system sleep while local transcription, model downloads, or
    // note generation are using the sidecar. The assertion releases on every
    // return path, including cancellation and timeout.
    let _sleep_assertion = matches!(
        operation_type.as_str(),
        "transcribe" | "generate_note" | "generate_notes" | "download_model"
    )
    .then(|| audio::recorder::SleepAssertion::acquire("Gist is processing session data"));
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if !s.started {
            return Err("Sidecar not running".into());
        }
        if s.busy {
            return Err("sidecar_busy".into());
        }
        let req_tx = match &s.request_tx {
            Some(tx) => tx.clone(),
            None => return Err("Sidecar not running".into()),
        };
        let log_file = s.sidecar_log.clone();
        s.busy = true;
        s.response_tx = Some((request_id.clone(), tx));

        if req_tx.send(request).is_err() {
            log_sidecar_event(
                log_file.as_ref(),
                "ERROR",
                format!(
                    "event=rpc_request_send_failed operation={} request_id={}",
                    operation_type, request_id
                ),
            );
            s.busy = false;
            s.response_tx.take();
            return Err("Failed to send request to sidecar".into());
        }

        log_sidecar_event(
            log_file.as_ref(),
            "INFO",
            format!(
                "event=rpc_request_sent operation={} request_id={}",
                operation_type, request_id
            ),
        );
    }

    emit_sidecar_state(&app, true);

    let result = match tokio::time::timeout(rpc_timeout(&operation_type), &mut rx).await {
        Ok(Ok(result)) => {
            let level = if result.is_ok() { "INFO" } else { "ERROR" };
            let log_file = state.lock().ok().and_then(|s| s.sidecar_log.clone());
            log_sidecar_event(
                log_file.as_ref(),
                level,
                format!(
                    "event=rpc_request_completed operation={} request_id={} success={}",
                    operation_type,
                    request_id,
                    result.is_ok()
                ),
            );
            result
        }
        Ok(Err(_)) => {
            let mut s = state.lock().map_err(|e| e.to_string())?;
            let log_file = s.sidecar_log.clone();
            if s.response_tx.as_ref().map(|(id, _)| id) == Some(&request_id) {
                s.response_tx.take();
                s.busy = false;
            }
            log_sidecar_event(
                log_file.as_ref(),
                "ERROR",
                format!(
                    "event=rpc_response_channel_closed operation={} request_id={}",
                    operation_type, request_id
                ),
            );
            emit_sidecar_state(&app, false);
            Err("Sidecar response channel closed".into())
        }
        Err(_) => {
            let cancel_tx = {
                let s = state.lock().map_err(|e| e.to_string())?;
                s.request_tx.clone()
            };
            if let Some(tx) = cancel_tx {
                let _ = tx.send(cancel_message(&request_id));
            }
            let log_file = state.lock().ok().and_then(|s| s.sidecar_log.clone());
            log_sidecar_event(
                log_file.as_ref(),
                "ERROR",
                format!(
                    "event=rpc_request_timed_out operation={} request_id={}",
                    operation_type, request_id
                ),
            );

            // Give cooperative MLX cancellation a short chance to unwind. If it
            // remains stuck, stop the process; ensureSidecar will start a fresh
            // one before the next operation.
            if tokio::time::timeout(std::time::Duration::from_secs(5), &mut rx)
                .await
                .is_err()
            {
                let mut s = state.lock().map_err(|e| e.to_string())?;
                s.request_tx.take();
                s.response_tx.take();
                if let Some(mut child) = s.child.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                s.started = false;
                s.busy = false;
                s.sidecar_log = None;
            }
            emit_sidecar_state(&app, false);
            Err("Sidecar operation timed out".into())
        }
    };

    result
}

#[tauri::command]
async fn developer_features_enabled() -> bool {
    developer_features_available()
}

#[tauri::command]
async fn cancel_sidecar(
    app: AppHandle,
    state: State<'_, SharedSidecarState>,
) -> Result<(), String> {
    let active_request_id = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let active_request_id = s.response_tx.as_ref().map(|(id, _)| id.clone());
        if let Some(tx) = &s.request_tx {
            let message = active_request_id
                .as_deref()
                .map(cancel_message)
                .unwrap_or_else(|| r#"{"type":"cancel"}"#.to_string());
            let _ = tx.send(message);
        }
        active_request_id
    };

    let log_file = state.lock().ok().and_then(|s| s.sidecar_log.clone());
    log_sidecar_event(
        log_file.as_ref(),
        "INFO",
        format!(
            "event=sidecar_cancel_requested request_id={}",
            active_request_id.as_deref().unwrap_or("none")
        ),
    );

    let Some(active_request_id) = active_request_id else {
        return Ok(());
    };

    // MLX normally observes cancellation between generated tokens. Prompt
    // prefill can hold control for several seconds, so wait for the exact
    // request to unwind before considering the sidecar unhealthy. The bounded
    // fallback protects the app from a genuinely stuck native MLX operation.
    let deadline = std::time::Instant::now() + SIDECAR_CANCEL_GRACE;
    loop {
        let request_still_active = {
            let s = state.lock().map_err(|e| e.to_string())?;
            s.response_tx
                .as_ref()
                .map(|(id, _)| id == &active_request_id)
                .unwrap_or(false)
        };

        if !request_still_active {
            return Ok(());
        }

        if std::time::Instant::now() < deadline {
            tokio::time::sleep(SIDECAR_CANCEL_POLL).await;
            continue;
        }

        let should_restart = {
            let mut s = state.lock().map_err(|e| e.to_string())?;
            if !s
                .response_tx
                .as_ref()
                .map(|(id, _)| id == &active_request_id)
                .unwrap_or(false)
            {
                false
            } else {
                s.request_tx.take();
                if let Some((_, tx)) = s.response_tx.take() {
                    let _ = tx.send(Err("Operation cancelled".into()));
                }
                if let Some(mut child) = s.child.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                s.started = false;
                s.busy = false;
                s.sidecar_log = None;
                true
            }
        };

        if should_restart {
            log_sidecar_event(
                log_file.as_ref(),
                "WARN",
                format!(
                    "event=sidecar_cancel_forced request_id={} grace_seconds={}",
                    active_request_id,
                    SIDECAR_CANCEL_GRACE.as_secs()
                ),
            );
            emit_sidecar_state(&app, false);

            match start_sidecar_process(&app, state.inner()) {
                Ok(_) => {
                    log_sidecar_event(
                        log_file.as_ref(),
                        "INFO",
                        format!(
                            "event=sidecar_restarted_after_cancel request_id={}",
                            active_request_id
                        ),
                    );
                }
                Err(error) => {
                    log_sidecar_event(
                        log_file.as_ref(),
                        "ERROR",
                        format!(
                            "event=sidecar_restart_after_cancel_failed request_id={} error_type=start_sidecar",
                            active_request_id,
                        ),
                    );
                    return Err(format!(
                        "Operation cancelled, but the processing engine could not restart: {}",
                        error
                    ));
                }
            }
        }
        return Ok(());
    }
}

#[tauri::command]
async fn is_running(state: State<'_, SharedSidecarState>) -> Result<bool, String> {
    match state.lock() {
        Ok(s) => Ok(s.started),
        Err(e) => Err(e.to_string()),
    }
}

// ── Patient CRUD ──────────────────────────────────────────────────────────

#[tauri::command]
async fn list_patients(db: State<'_, Mutex<Database>>) -> Result<Vec<Patient>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .conn
        .prepare("SELECT id, name, created_at FROM patients ORDER BY created_at DESC")
        .map_err(|e| e.to_string())?;
    let patients = stmt
        .query_map([], |row| {
            Ok(Patient {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(patients)
}

#[tauri::command]
async fn create_patient(
    db: State<'_, Mutex<Database>>,
    data: CreatePatient,
) -> Result<Patient, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let now = Local::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO patients (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id, data.name, now],
        )
        .map_err(|e| e.to_string())?;
    Ok(Patient {
        id,
        name: data.name,
        created_at: now,
    })
}

#[tauri::command]
async fn update_patient(db: State<'_, Mutex<Database>>, data: UpdatePatient) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let affected = db
        .conn
        .execute(
            "UPDATE patients SET name = ?1 WHERE id = ?2",
            params![data.name, data.id],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("Patient not found".into());
    }
    Ok(())
}

#[tauri::command]
async fn delete_patient(
    app: AppHandle,
    db: State<'_, Mutex<Database>>,
    id: String,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let paths = audio_paths_for_patient(&db.conn, &id)?;
    let session_ids = {
        let mut stmt = db
            .conn
            .prepare("SELECT id FROM sessions WHERE patient_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM sessions WHERE patient_id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM patients WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    drop(db);
    if let Err(error) = remove_managed_audio_files(&app, paths) {
        log::warn!("Patient was deleted, but managed audio cleanup failed: {error}");
    }
    for session_id in session_ids {
        if let Err(error) = remove_session_diagnostics(&app, &session_id) {
            log::warn!("Patient was deleted, but diagnostic cleanup failed: {error}");
        }
    }
    Ok(())
}

// ── Session CRUD ──────────────────────────────────────────────────────────

#[tauri::command]
async fn list_sessions(
    db: State<'_, Mutex<Database>>,
    patient_id: Option<String>,
) -> Result<Vec<Session>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let (sql, has_param) = if patient_id.is_some() {
        (format!("SELECT {} FROM sessions WHERE patient_id = ?1 ORDER BY date DESC, COALESCE(start_time, '') DESC, created_at DESC", SESSION_COLUMNS), true)
    } else {
        (format!("SELECT {} FROM sessions ORDER BY date DESC, COALESCE(start_time, '') DESC, created_at DESC", SESSION_COLUMNS), false)
    };
    let mut stmt = db.conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut sessions: Vec<Session> = if has_param {
        stmt.query_map(params![patient_id], map_session)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    } else {
        stmt.query_map([], map_session)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    for s in &mut sessions {
        s.inputs = fetch_session_inputs(&db.conn, &s.id)?;
        s.notes = fetch_session_notes(&db.conn, &s.id)?;
    }
    Ok(sessions)
}

#[tauri::command]
async fn create_session(
    db: State<'_, Mutex<Database>>,
    data: CreateSession,
) -> Result<Session, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let now = Local::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO sessions (id, patient_id, date, start_time, title, session_type, updated_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, data.patient_id, data.date, data.start_time, data.title, data.session_type, now, now],
        )
        .map_err(|e| e.to_string())?;
    Ok(Session {
        id,
        patient_id: data.patient_id,
        date: data.date,
        start_time: data.start_time,
        title: data.title,
        session_type: data.session_type,
        updated_at: Some(now.clone()),
        created_at: now,
        inputs: Vec::new(),
        notes: Vec::new(),
    })
}

#[tauri::command]
async fn update_session(db: State<'_, Mutex<Database>>, data: UpdateSession) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let now = Local::now().to_rfc3339();
    let affected = db.conn
        .execute(
            "UPDATE sessions SET date = ?1, start_time = ?2, title = ?3, session_type = ?4, updated_at = ?5 WHERE id = ?6",
            params![data.date, data.start_time, data.title, data.session_type, now, data.id],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("Session not found".into());
    }
    Ok(())
}

#[tauri::command]
async fn get_session(
    db: State<'_, Mutex<Database>>,
    id: String,
) -> Result<Option<Session>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let sql = format!("SELECT {} FROM sessions WHERE id = ?1", SESSION_COLUMNS);
    let mut stmt = db.conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![id], map_session)
        .map_err(|e| e.to_string())?;
    match rows.next() {
        Some(Ok(mut session)) => {
            session.inputs = fetch_session_inputs(&db.conn, &session.id)?;
            session.notes = fetch_session_notes(&db.conn, &session.id)?;
            Ok(Some(session))
        }
        Some(Err(e)) => Err(e.to_string()),
        None => Ok(None),
    }
}

#[tauri::command]
async fn delete_session(
    app: AppHandle,
    db: State<'_, Mutex<Database>>,
    id: String,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let paths = audio_paths_for_session(&db.conn, &id)?;
    db.conn
        .execute("DELETE FROM sessions WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    drop(db);
    if let Err(error) = remove_managed_audio_files(&app, paths) {
        log::warn!("Session was deleted, but managed audio cleanup failed: {error}");
    }
    if let Err(error) = remove_session_diagnostics(&app, &id) {
        log::warn!("Session was deleted, but diagnostic cleanup failed: {error}");
    }
    Ok(())
}

#[tauri::command]
async fn create_session_input(
    db: State<'_, Mutex<Database>>,
    data: CreateSessionInput,
) -> Result<SessionInput, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let now = Local::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO session_inputs (
                id, session_id, kind, source, title, text, audio_file, duration_seconds,
                transcription_model, include_in_notes, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                id,
                data.session_id,
                data.kind,
                data.source,
                data.title,
                data.text,
                data.audio_file,
                data.duration_seconds,
                data.transcription_model,
                if data.include_in_notes { 1 } else { 0 },
                now
            ],
        )
        .map_err(|e| e.to_string())?;

    get_session_input_by_id(&db.conn, &id)
}

#[tauri::command]
async fn update_session_input(
    db: State<'_, Mutex<Database>>,
    data: UpdateSessionInput,
) -> Result<SessionInput, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let existing = get_session_input_by_id(&db.conn, &data.id)?;
    let title = data.title.unwrap_or(existing.title);
    let text = data.text.unwrap_or(existing.text);
    let include_in_notes = data.include_in_notes.unwrap_or(existing.include_in_notes);
    let updated_at = Local::now().to_rfc3339();
    let affected = db
        .conn
        .execute(
            "UPDATE session_inputs
             SET title = ?1, text = ?2, include_in_notes = ?3, updated_at = ?4
             WHERE id = ?5",
            params![
                title,
                text,
                if include_in_notes { 1 } else { 0 },
                updated_at,
                data.id
            ],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("Session input not found".into());
    }
    db.conn
        .execute(
            "DELETE FROM evidence_ledger_cache WHERE source_id = ?1",
            params![data.id],
        )
        .map_err(|e| e.to_string())?;
    get_session_input_by_id(&db.conn, &data.id)
}

#[tauri::command]
async fn delete_session_input(
    app: AppHandle,
    db: State<'_, Mutex<Database>>,
    id: String,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let audio_file: Option<String> = db
        .conn
        .query_row(
            "SELECT audio_file FROM session_inputs WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let affected = db
        .conn
        .execute("DELETE FROM session_inputs WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("Session input not found".into());
    }
    drop(db);
    if let Some(path) = audio_file {
        if let Err(error) = remove_managed_audio_files(&app, vec![path]) {
            log::warn!("Source was deleted, but managed audio cleanup failed: {error}");
        }
    }
    Ok(())
}

fn get_session_input_by_id(conn: &Connection, id: &str) -> Result<SessionInput, String> {
    conn.query_row(
        "SELECT id, session_id, kind, source, title, text, audio_file, duration_seconds, transcription_model, include_in_notes, created_at, updated_at
         FROM session_inputs
         WHERE id = ?1",
        params![id],
        |row| {
            Ok(SessionInput {
                id: row.get(0)?,
                session_id: row.get(1)?,
                kind: row.get(2)?,
                source: row.get(3)?,
                title: row.get(4)?,
                text: row.get(5)?,
                audio_file: row.get(6)?,
                duration_seconds: row.get(7)?,
                transcription_model: row.get(8)?,
                include_in_notes: row.get::<_, i64>(9)? != 0,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_session_note(
    db: State<'_, Mutex<Database>>,
    session_id: String,
    format: String,
    note: String,
    llm_model: Option<String>,
) -> Result<SessionNote, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let now = Local::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO session_notes (id, session_id, format, note, llm_model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(session_id, format) DO UPDATE SET note = ?4, llm_model = ?5, created_at = ?6",
            params![id, session_id, format, note, llm_model, now],
        )
        .map_err(|e| e.to_string())?;
    // Fetch the actual row back so id/created_at are correct on conflict
    let row = db.conn
        .query_row(
            "SELECT id, session_id, format, note, llm_model, created_at FROM session_notes WHERE session_id = ?1 AND format = ?2",
            params![session_id, format],
            |row| Ok(SessionNote {
                id: row.get(0)?,
                session_id: row.get(1)?,
                format: row.get(2)?,
                note: row.get(3)?,
                llm_model: row.get(4)?,
                created_at: row.get(5)?,
            }),
        )
        .map_err(|e| e.to_string())?;
    Ok(row)
}

#[tauri::command]
async fn get_patient_formats(
    db: State<'_, Mutex<Database>>,
    patient_id: String,
) -> Result<Vec<String>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let key = format!("patient_formats_{}", patient_id);
    let mut stmt = db
        .conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![key], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    match rows.next() {
        Some(Ok(v)) => serde_json::from_str::<Vec<String>>(&v)
            .or_else(|_| {
                // Migrate values written by versions that used comma-delimited
                // storage. New values are JSON so custom names can contain commas.
                Ok::<Vec<String>, serde_json::Error>(
                    v.split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect(),
                )
            })
            .map_err(|e| e.to_string()),
        _ => Ok(Vec::new()),
    }
}

#[tauri::command]
async fn set_patient_formats(
    db: State<'_, Mutex<Database>>,
    patient_id: String,
    formats: Vec<String>,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let key = format!("patient_formats_{}", patient_id);
    let value = serde_json::to_string(&formats).map_err(|e| e.to_string())?;
    db.conn
        .execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Settings ──────────────────────────────────────────────────────────────

#[tauri::command]
async fn get_setting(
    db: State<'_, Mutex<Database>>,
    key: String,
) -> Result<Option<String>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![key], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e.to_string()),
        None => Ok(None),
    }
}

#[tauri::command]
async fn set_setting(
    db: State<'_, Mutex<Database>>,
    key: String,
    value: String,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.conn
        .execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn total_memory_bytes() -> Result<u64, String> {
    #[cfg(target_os = "macos")]
    {
        let name = CString::new("hw.memsize").map_err(|e| e.to_string())?;
        let mut bytes = 0_u64;
        let mut size = std::mem::size_of::<u64>();
        let result = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&mut bytes as *mut u64).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 || size != std::mem::size_of::<u64>() || bytes == 0 {
            return Err("Could not determine system memory".into());
        }
        return Ok(bytes);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if pages <= 0 || page_size <= 0 {
            return Err("Could not determine system memory".into());
        }
        return (pages as u64)
            .checked_mul(page_size as u64)
            .ok_or_else(|| "Could not determine system memory".into());
    }

    #[cfg(not(unix))]
    {
        Err("System memory detection is unavailable on this platform".into())
    }
}

#[tauri::command]
async fn get_system_memory_bytes() -> Result<u64, String> {
    total_memory_bytes()
}

// ── Note Format Templates ─────────────────────────────────────────────────

fn map_note_format(row: &Row) -> rusqlite::Result<NoteFormatTemplate> {
    Ok(NoteFormatTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        prompt: row.get(2)?,
        is_builtin: row.get::<_, i64>(3)? != 0,
        hidden: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
    })
}

#[tauri::command]
async fn list_note_formats(
    db: State<'_, Mutex<Database>>,
) -> Result<Vec<NoteFormatTemplate>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db.conn
        .prepare("SELECT id, name, prompt, is_builtin, hidden, created_at FROM note_formats ORDER BY hidden ASC, is_builtin DESC, name ASC")
        .map_err(|e| e.to_string())?;
    let formats = stmt
        .query_map([], map_note_format)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(formats)
}

#[tauri::command]
async fn create_note_format(
    db: State<'_, Mutex<Database>>,
    data: CreateNoteFormat,
) -> Result<NoteFormatTemplate, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let now = Local::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO note_formats (id, name, prompt, is_builtin, hidden, created_at) VALUES (?1, ?2, ?3, 0, 0, ?4)",
            params![id, data.name, data.prompt, now],
        )
        .map_err(|e| e.to_string())?;
    Ok(NoteFormatTemplate {
        id,
        name: data.name,
        prompt: data.prompt,
        is_builtin: false,
        hidden: false,
        created_at: now,
    })
}

#[tauri::command]
async fn update_note_format(
    db: State<'_, Mutex<Database>>,
    data: UpdateNoteFormat,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // Get old name to cascade rename in session_notes
    let old_name: Option<String> = tx
        .query_row(
            "SELECT name FROM note_formats WHERE id = ?1",
            params![data.id],
            |row| row.get(0),
        )
        .ok();
    let affected = tx
        .execute(
            "UPDATE note_formats SET name = ?1, prompt = ?2 WHERE id = ?3",
            params![data.name, data.prompt, data.id],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("Format not found".into());
    }
    // Cascade rename session_notes if name changed
    if let Some(old) = old_name {
        if old != data.name {
            tx.execute(
                "UPDATE session_notes SET format = ?1 WHERE format = ?2",
                params![data.name, old],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn delete_note_format(db: State<'_, Mutex<Database>>, id: String) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    // Refuse to delete built-in formats — they can only be reset or hidden.
    let is_builtin: i64 = db
        .conn
        .query_row(
            "SELECT is_builtin FROM note_formats WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if is_builtin != 0 {
        return Err("Built-in formats cannot be deleted. Use Reset or Hide instead.".into());
    }
    // Refuse to delete if session_notes reference this format
    let name: String = db
        .conn
        .query_row(
            "SELECT name FROM note_formats WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let note_count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM session_notes WHERE format = ?1",
            params![name],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if note_count > 0 {
        return Err(format!(
            "Cannot delete format '{}' — {} session note(s) reference it. Remove those notes first.",
            name, note_count
        ));
    }
    db.conn
        .execute("DELETE FROM note_formats WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn reset_note_format(db: State<'_, Mutex<Database>>, id: String) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let name: String = db
        .conn
        .query_row(
            "SELECT name FROM note_formats WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let default_prompt = default_formats()
        .into_iter()
        .find(|(default_name, _)| default_name == &name)
        .map(|(_, prompt)| prompt);
    match default_prompt {
        Some(prompt) => {
            db.conn
                .execute(
                    "UPDATE note_formats SET prompt = ?1 WHERE id = ?2",
                    params![prompt, id],
                )
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        None => Err(format!("No default prompt for format '{}'", name)),
    }
}

#[tauri::command]
async fn toggle_note_format_hidden(
    db: State<'_, Mutex<Database>>,
    id: String,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.conn
        .execute(
            "UPDATE note_formats SET hidden = NOT hidden WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── File Dialog ───────────────────────────────────────────────────────────

fn note_generation_diagnostics_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("note-generation-diagnostics"))
}

fn remove_session_diagnostics(app: &AppHandle, session_id: &str) -> Result<(), String> {
    let directory = note_generation_diagnostics_dir(app)?.join(session_id);
    if directory.exists() {
        std::fs::remove_dir_all(directory).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn export_session_diagnostics(
    app: AppHandle,
    session_id: String,
) -> Result<Option<DiagnosticExportResult>, String> {
    if !developer_features_available() {
        return Err("Developer diagnostics are unavailable in release builds".into());
    }
    Uuid::parse_str(&session_id).map_err(|_| "Invalid session identifier".to_string())?;
    let directory = note_generation_diagnostics_dir(&app)?.join(&session_id);
    let mut run_paths = if directory.exists() {
        std::fs::read_dir(&directory)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    run_paths.sort();
    if run_paths.is_empty() {
        return Err("No diagnostic runs have been captured for this session".into());
    }
    let runs = run_paths
        .iter()
        .map(|path| {
            let file = File::open(path).map_err(|e| e.to_string())?;
            serde_json::from_reader::<_, Value>(file).map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let export = serde_json::json!({
        "schema_version": 1,
        "exported_at": Local::now().to_rfc3339(),
        "session_id": session_id,
        "contains_sensitive_clinical_data": true,
        "runs": runs,
    });
    let contents = serde_json::to_vec_pretty(&export).map_err(|e| e.to_string())?;

    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name(format!(
            "gist-session-diagnostics-{}.json",
            &session_id[..8]
        ))
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    file.write_all(&contents).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    Ok(Some(DiagnosticExportResult {
        path: path.to_string_lossy().into_owned(),
        run_count: run_paths.len(),
    }))
}

#[tauri::command]
async fn pick_audio_file(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter(
            "Audio",
            &["wav", "mp3", "m4a", "flac", "ogg", "aiff", "aac"],
        )
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let file = rx.await.map_err(|e| e.to_string())?;
    match file {
        Some(path) => {
            let p = path.into_path().map_err(|e| e.to_string())?;
            Ok(Some(p.to_string_lossy().into_owned()))
        }
        None => Ok(None),
    }
}

// ── Audio Recording ───────────────────────────────────────────────────────

fn recordings_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let directory = app_dir.join("recordings");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    Ok(directory)
}

const TWO_HOUR_RECORDING_BYTES: u64 = 48_000 * 2 * 60 * 60 * 2;
const RECORDING_FREE_SPACE_RESERVE: u64 = 128 * 1024 * 1024;

#[cfg(target_os = "macos")]
fn available_disk_space(path: &Path) -> Result<u64, String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "Could not check available storage for recording".to_string())?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
    if result != 0 {
        return Err("Could not check available storage for recording".into());
    }
    Ok((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(not(target_os = "macos"))]
fn available_disk_space(_path: &Path) -> Result<u64, String> {
    Ok(u64::MAX)
}

fn ensure_recording_space(app: &AppHandle) -> Result<(), String> {
    let directory = recordings_dir(app)?;
    std::fs::create_dir_all(&directory)
        .map_err(|_| "Gist could not prepare its recordings folder.".to_string())?;
    let available = available_disk_space(&directory)?;
    let needed = TWO_HOUR_RECORDING_BYTES + RECORDING_FREE_SPACE_RESERVE;
    if available < needed {
        return Err("Not enough free storage to safely record a two-hour session. Free at least 800 MB, then try again.".into());
    }
    Ok(())
}

fn map_recording_job(row: &Row) -> rusqlite::Result<RecordingJob> {
    let formats_json: String = row.get(4)?;
    let formats = serde_json::from_str(&formats_json).unwrap_or_default();
    Ok(RecordingJob {
        id: row.get(0)?,
        session_id: row.get(1)?,
        audio_file: row.get(2)?,
        input_kind: row.get(3)?,
        formats,
        llm_model: row.get(5)?,
        thinking: row.get::<_, i64>(6)? != 0,
        diarize: row.get::<_, i64>(7)? != 0,
        num_speakers: row.get(8)?,
        created_session: row.get::<_, i64>(9)? != 0,
        state: row.get(10)?,
        error: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

const RECORDING_JOB_COLUMNS: &str = "id, session_id, audio_file, input_kind, formats_json, llm_model, thinking, diarize, num_speakers, created_session, state, error, created_at, updated_at";

fn get_recording_job_by_id(conn: &Connection, id: &str) -> Result<RecordingJob, String> {
    let sql = format!(
        "SELECT {} FROM recording_jobs WHERE id = ?1",
        RECORDING_JOB_COLUMNS
    );
    conn.query_row(&sql, params![id], map_recording_job)
        .map_err(|e| e.to_string())
}

fn repair_partial_wav(path: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() < 44 {
        return Err("The interrupted recording is too short to recover.".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut header = [0_u8; 12];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err("The interrupted recording is not a WAV file Gist can recover.".into());
    }
    let riff_size = (metadata.len() - 8) as u32;
    let data_size = (metadata.len() - 44) as u32;
    file.seek(SeekFrom::Start(4)).map_err(|e| e.to_string())?;
    file.write_all(&riff_size.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(40)).map_err(|e| e.to_string())?;
    file.write_all(&data_size.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

fn managed_recording_path(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    let root = recordings_dir(app)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let candidate = PathBuf::from(path);
    let parent = candidate.parent().ok_or("Invalid recording path")?;
    let canonical_parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if canonical_parent != root {
        return Err(
            "Refusing to operate on an audio file outside Gist's recordings folder.".into(),
        );
    }
    Ok(candidate)
}

fn audio_paths_for_session(conn: &Connection, session_id: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT audio_file FROM session_inputs WHERE session_id = ?1 AND audio_file IS NOT NULL
         UNION
         SELECT audio_file FROM recording_jobs WHERE session_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let paths = stmt
        .query_map(params![session_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(paths)
}

fn audio_paths_for_patient(conn: &Connection, patient_id: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT si.audio_file FROM session_inputs si
         JOIN sessions s ON s.id = si.session_id
         WHERE s.patient_id = ?1 AND si.audio_file IS NOT NULL
         UNION
         SELECT r.audio_file FROM recording_jobs r
         JOIN sessions s ON s.id = r.session_id
         WHERE s.patient_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let paths = stmt
        .query_map(params![patient_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(paths)
}

fn remove_managed_audio_files(app: &AppHandle, paths: Vec<String>) -> Result<(), String> {
    let mut failures = Vec::new();
    for source in paths {
        let Ok(path) = managed_recording_path(app, &source) else {
            continue;
        };
        if path.exists() {
            if let Err(error) = std::fs::remove_file(&path) {
                failures.push(format!("{} ({})", path.display(), error));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "The record was deleted, but Gist could not remove its managed audio file(s): {}",
            failures.join(", ")
        ))
    }
}

#[tauri::command]
async fn list_recoverable_recording_jobs(
    app: AppHandle,
    db: State<'_, Mutex<Database>>,
) -> Result<Vec<RecordingJob>, String> {
    let active_job_id = audio::recorder::get_recording_state().job_id;
    let db = db.lock().map_err(|e| e.to_string())?;
    let sql = format!(
        "SELECT {} FROM recording_jobs WHERE state IN ('recording', 'recorded', 'failed') ORDER BY created_at ASC",
        RECORDING_JOB_COLUMNS
    );
    let mut stmt = db.conn.prepare(&sql).map_err(|e| e.to_string())?;
    let jobs = stmt
        .query_map([], map_recording_job)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let now = Local::now().to_rfc3339();
    let mut recovered = Vec::with_capacity(jobs.len());
    for mut job in jobs {
        if job.state == "recording" && active_job_id.as_deref() == Some(job.id.as_str()) {
            continue;
        }
        if job.state == "recording" {
            let partial_path = managed_recording_path(&app, &job.audio_file)?;
            match repair_partial_wav(&partial_path) {
                Ok(()) => {
                    let final_path =
                        partial_path.with_file_name(format!("recording_{}.wav", job.id));
                    std::fs::rename(&partial_path, &final_path).map_err(|e| e.to_string())?;
                    job.audio_file = final_path.to_string_lossy().into_owned();
                    job.state = "recorded".into();
                    job.error = Some("Recording was recovered after Gist closed unexpectedly. Please review it before processing.".into());
                    job.updated_at = now.clone();
                    db.conn.execute(
                        "UPDATE recording_jobs SET audio_file = ?1, state = ?2, error = ?3, updated_at = ?4 WHERE id = ?5",
                        params![job.audio_file, job.state, job.error, job.updated_at, job.id],
                    ).map_err(|e| e.to_string())?;
                }
                Err(error) => {
                    job.state = "failed".into();
                    job.error = Some(error);
                    job.updated_at = now.clone();
                    db.conn.execute(
                        "UPDATE recording_jobs SET state = ?1, error = ?2, updated_at = ?3 WHERE id = ?4",
                        params![job.state, job.error, job.updated_at, job.id],
                    ).map_err(|e| e.to_string())?;
                }
            }
        }
        recovered.push(job);
    }
    Ok(recovered)
}

#[tauri::command]
async fn get_recording_job(
    db: State<'_, Mutex<Database>>,
    id: String,
) -> Result<RecordingJob, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    get_recording_job_by_id(&db.conn, &id)
}

#[tauri::command]
async fn complete_recording_job(db: State<'_, Mutex<Database>>, id: String) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.conn.execute(
        "UPDATE recording_jobs SET state = 'completed', error = NULL, updated_at = ?1 WHERE id = ?2",
        params![Local::now().to_rfc3339(), id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn set_recording_job_error(
    db: State<'_, Mutex<Database>>,
    id: String,
    error: String,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.conn.execute(
        "UPDATE recording_jobs SET state = 'recorded', error = ?1, updated_at = ?2 WHERE id = ?3",
        params![error, Local::now().to_rfc3339(), id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn discard_recording_job(
    app: AppHandle,
    db: State<'_, Mutex<Database>>,
    id: String,
) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let job = get_recording_job_by_id(&db.conn, &id)?;
    let path = managed_recording_path(&app, &job.audio_file)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    db.conn
        .execute("DELETE FROM recording_jobs WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if job.created_session {
        db.conn.execute(
            "DELETE FROM sessions WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM session_inputs WHERE session_id = ?1) AND NOT EXISTS (SELECT 1 FROM session_notes WHERE session_id = ?1)",
            params![job.session_id],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn list_audio_devices() -> Result<Vec<audio::AudioDeviceInfo>, String> {
    // CoreAudio/CPAL may enumerate no inputs (or later deliver only silence)
    // when TCC access is still undetermined. Ask explicitly before presenting
    // the device list so first use reliably triggers the macOS permission UI.
    audio::permissions::ensure_microphone_access()
        .await
        .map_err(|e| e.to_string())?;
    audio::list_audio_devices().map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_recording(
    app: AppHandle,
    db: State<'_, Mutex<Database>>,
    data: StartRecordingData,
    mic_device: Option<String>,
    system_device: Option<String>,
) -> Result<RecordingJob, String> {
    // Keep this check at the capture boundary as well as device enumeration:
    // callers can retain a previously selected device or invoke this command
    // without listing devices first.
    audio::permissions::ensure_microphone_access()
        .await
        .map_err(|e| e.to_string())?;
    ensure_recording_space(&app)?;
    let num_speakers = validate_num_speakers(data.num_speakers)?;
    let id = Uuid::new_v4().to_string();
    let partial_path = recordings_dir(&app)?.join(format!("recording_{}.partial.wav", id));
    let now = Local::now().to_rfc3339();
    let formats_json = serde_json::to_string(&data.formats).map_err(|e| e.to_string())?;
    {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.conn.execute(
            "INSERT INTO recording_jobs (id, session_id, audio_file, input_kind, formats_json, llm_model, thinking, diarize, num_speakers, created_session, state, error, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'recording', NULL, ?11, ?11)",
            params![
                id, data.session_id, partial_path.to_string_lossy(), data.input_kind, formats_json,
                data.llm_model, if data.thinking { 1 } else { 0 }, if data.diarize { 1 } else { 0 },
                num_speakers, if data.created_session { 1 } else { 0 }, now
            ],
        ).map_err(|e| e.to_string())?;
    }
    if let Err(error) = audio::recorder::start_recording(
        app.clone(),
        mic_device,
        system_device,
        id.clone(),
        partial_path.clone(),
    ) {
        let db = db.lock().map_err(|e| e.to_string())?;
        let _ = db.conn.execute(
            "UPDATE recording_jobs SET state = 'failed', error = ?1, updated_at = ?2 WHERE id = ?3",
            params![error.to_string(), Local::now().to_rfc3339(), id],
        );
        return Err(error.to_string());
    }
    let db = db.lock().map_err(|e| e.to_string())?;
    get_recording_job_by_id(&db.conn, &id)
}

#[tauri::command]
async fn stop_recording(
    app: AppHandle,
    db: State<'_, Mutex<Database>>,
) -> Result<audio::recorder::StopRecordingResult, String> {
    let result = audio::recorder::stop_recording(app.clone()).map_err(|e| e.to_string())?;
    let partial_path = managed_recording_path(&app, &result.file_path)?;
    let final_path = partial_path.with_file_name(format!("recording_{}.wav", result.job_id));
    std::fs::rename(&partial_path, &final_path).map_err(|e| e.to_string())?;
    let db = db.lock().map_err(|e| e.to_string())?;
    db.conn.execute(
        "UPDATE recording_jobs SET audio_file = ?1, state = 'recorded', error = NULL, updated_at = ?2 WHERE id = ?3",
        params![final_path.to_string_lossy(), Local::now().to_rfc3339(), result.job_id],
    ).map_err(|e| e.to_string())?;
    let finalized = audio::recorder::StopRecordingResult {
        job_id: result.job_id,
        file_path: final_path.to_string_lossy().into_owned(),
        duration_seconds: result.duration_seconds,
        is_short_recording: result.is_short_recording,
    };
    app.emit("recording-stopped", &finalized)
        .map_err(|e| e.to_string())?;
    Ok(finalized)
}

#[tauri::command]
async fn pause_recording(app: AppHandle) -> Result<(), String> {
    audio::recorder::pause_recording(app).map_err(|e| e.to_string())
}

#[tauri::command]
async fn resume_recording(app: AppHandle) -> Result<(), String> {
    audio::recorder::resume_recording(app).map_err(|e| e.to_string())
}

#[tauri::command]
async fn is_recording() -> bool {
    audio::recorder::is_recording()
}

#[tauri::command]
async fn get_recording_state() -> audio::recorder::RecordingStatePayload {
    audio::recorder::get_recording_state()
}

// ── App Entry ─────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let db = Database::new(&handle).map_err(|e| {
                log::error!(target: "gist.database", "event=database_initialization_failed");
                e
            })?;
            app.manage(Mutex::new(db));
            app.manage(Arc::new(Mutex::new(SidecarState {
                request_tx: None,
                response_tx: None,
                child: None,
                sidecar_log: None,
                generation: 0,
                started: false,
                busy: false,
            })));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_sidecar,
            stop_sidecar,
            rpc_call,
            is_running,
            cancel_sidecar,
            developer_features_enabled,
            list_patients,
            create_patient,
            update_patient,
            delete_patient,
            list_sessions,
            create_session,
            update_session,
            get_session,
            delete_session,
            create_session_input,
            update_session_input,
            delete_session_input,
            create_session_note,
            get_patient_formats,
            set_patient_formats,
            get_setting,
            set_setting,
            get_system_memory_bytes,
            pick_audio_file,
            export_session_diagnostics,
            list_note_formats,
            create_note_format,
            update_note_format,
            delete_note_format,
            reset_note_format,
            toggle_note_format_hidden,
            list_recoverable_recording_jobs,
            get_recording_job,
            complete_recording_job,
            set_recording_job_error,
            discard_recording_job,
            list_audio_devices,
            start_recording,
            stop_recording,
            pause_recording,
            resume_recording,
            is_recording,
            get_recording_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

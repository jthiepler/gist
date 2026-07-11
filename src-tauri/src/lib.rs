use chrono::Local;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

mod audio;

// ── Database ──────────────────────────────────────────────────────────────

const SESSION_COLUMNS: &str = "id, patient_id, date, created_at";

fn map_session(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        date: row.get(2)?,
        created_at: row.get(3)?,
        inputs: Vec::new(),
        notes: Vec::new(),
    })
}

fn fetch_session_inputs(conn: &Connection, session_id: &str) -> Vec<SessionInput> {
    let mut stmt = match conn.prepare(
        "SELECT id, session_id, kind, source, title, text, audio_file, duration_seconds, language, transcription_model, include_in_notes, created_at, updated_at
         FROM session_inputs
         WHERE session_id = ?1
         ORDER BY created_at ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![session_id], |row| {
        Ok(SessionInput {
            id: row.get(0)?,
            session_id: row.get(1)?,
            kind: row.get(2)?,
            source: row.get(3)?,
            title: row.get(4)?,
            text: row.get(5)?,
            audio_file: row.get(6)?,
            duration_seconds: row.get(7)?,
            language: row.get(8)?,
            transcription_model: row.get(9)?,
            include_in_notes: row.get::<_, i64>(10)? != 0,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

fn fetch_session_notes(conn: &Connection, session_id: &str) -> Vec<SessionNote> {
    let mut stmt = match conn.prepare(
        "SELECT id, session_id, format, note, llm_model, created_at FROM session_notes WHERE session_id = ?1 ORDER BY format ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![session_id], |row| {
        Ok(SessionNote {
            id: row.get(0)?,
            session_id: row.get(1)?,
            format: row.get(2)?,
            note: row.get(3)?,
            llm_model: row.get(4)?,
            created_at: row.get(5)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
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
                language TEXT,
                transcription_model TEXT,
                include_in_notes INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
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
            );",
        )
        .map_err(|e| e.to_string())?;

        let _ = conn.execute("ALTER TABLE note_formats ADD COLUMN hidden INTEGER DEFAULT 0", []);

        // Seed only missing defaults. Existing built-ins may have been edited by
        // the user and are reset explicitly through the templates UI.
        let now = Local::now().to_rfc3339();
        for (name, prompt) in default_formats() {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM note_formats WHERE name = ?1",
                    params![name],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if exists == 0 {
                let id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO note_formats (id, name, prompt, is_builtin, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
                    params![id, name, prompt, now],
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
    language: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NoteFormatTemplate {
    id: String,
    name: String,
    prompt: String,
    is_builtin: bool,
    hidden: bool,
    created_at: String,
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
}

#[derive(Debug, Deserialize)]
struct UpdateSession {
    id: String,
    date: String,
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
    language: Option<String>,
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
    common_rules: Vec<String>,
    formats: Vec<DefaultFormat>,
}

fn default_formats() -> Vec<(String, String)> {
    let catalog: DefaultFormatCatalog = serde_json::from_str(include_str!("../../gist/formats/defaults.json"))
        .expect("bundled clinical note format defaults must be valid JSON");
    let DefaultFormatCatalog { common_rules, formats } = catalog;
    formats
        .into_iter()
        .map(|format| {
            let mut prompt = format!(
                "{}. Generate a clinical note from the labeled source materials.\n\nRules:\n",
                format.description
            );
            for rule in &common_rules {
                prompt.push_str("- ");
                prompt.push_str(rule);
                prompt.push('\n');
            }
            prompt.push_str("\nOutput format:\n\n");
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
    response_tx: Option<oneshot::Sender<Result<Value, String>>>,
    child: Option<Child>,
    started: bool,
    busy: bool,
}

type SharedSidecarState = Arc<Mutex<SidecarState>>;

fn emit_sidecar_state(app: &AppHandle, busy: bool) {
    let _ = app.emit("sidecar-state", serde_json::json!({ "busy": busy }));
}

// ── Sidecar Commands ──────────────────────────────────────────────────────

#[tauri::command]
async fn start_sidecar(app: AppHandle, state: State<'_, SharedSidecarState>) -> Result<String, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    if s.started {
        return Err("Sidecar already running".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let sidecar_path = resource_dir.join("resources").join("gist-sidecar").join("gist-sidecar");

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
    let mlx_lib_dir = sidecar_path.parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("_internal/mlx/lib");
    let dyld_path = std::env::var("DYLD_FALLBACK_LIBRARY_PATH").unwrap_or_default();
    let dyld_path = if dyld_path.is_empty() {
        mlx_lib_dir.to_string_lossy().into_owned()
    } else {
        format!("{}:{}", mlx_lib_dir.display(), dyld_path)
    };

    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let log_path = app_dir.join("sidecar.log");
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .map(File::from)
        .map(Stdio::from)
        .unwrap_or(Stdio::null());

    let mut child = Command::new(&sidecar_path)
        .arg("serve")
        .env("DYLD_FALLBACK_LIBRARY_PATH", &dyld_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .map_err(|e| format!("Failed to start sidecar: {}", e))?;

    let stdin = child.stdin.take().ok_or("Failed to capture stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;

    let (req_tx, req_rx) = mpsc::unbounded_channel::<String>();

    // Writer task: owns stdin, drains request channel
    std::thread::spawn(move || {
        let mut stdin = stdin;
        let mut rx = req_rx;
        while let Some(line) = rx.blocking_recv() {
            if writeln!(stdin, "{}", line).is_err() {
                break;
            }
            if stdin.flush().is_err() {
                break;
            }
        }
        // stdin dropped here → sidecar gets EOF on its stdin
    });

    // Reader task: owns stdout, emits progress, routes responses
    let app_clone = app.clone();
    let state_clone: SharedSidecarState = state.inner().clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,  // EOF or error → sidecar died
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let parsed: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };

            match parsed.get("type").and_then(|v| v.as_str()) {
                Some("progress") => {
                    let _ = app_clone.emit("sidecar-progress", &parsed);
                }
                Some("result") | Some("pong") => {
                    let resp_tx = {
                        if let Ok(mut s) = state_clone.lock() {
                            s.response_tx.take()
                        } else {
                            None
                        }
                    };
                    if let Some(tx) = resp_tx {
                        let _ = tx.send(Ok(parsed));
                    }
                    if let Ok(mut s) = state_clone.lock() {
                        s.busy = false;
                    }
                    emit_sidecar_state(&app_clone, false);
                }
                Some("error") => {
                    let msg = parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();
                    let resp_tx = {
                        if let Ok(mut s) = state_clone.lock() {
                            s.response_tx.take()
                        } else {
                            None
                        }
                    };
                    if let Some(tx) = resp_tx {
                        let _ = tx.send(Err(msg));
                    }
                    if let Ok(mut s) = state_clone.lock() {
                        s.busy = false;
                    }
                    emit_sidecar_state(&app_clone, false);
                }
                _ => {}
            }
        }

        // EOF on stdout — sidecar died
        if let Ok(mut s) = state_clone.lock() {
            s.started = false;
            s.busy = false;
            if let Some(tx) = s.response_tx.take() {
                let _ = tx.send(Err("Sidecar closed connection unexpectedly".into()));
            }
        }
        emit_sidecar_state(&app_clone, false);
    });

    s.request_tx = Some(req_tx);
    s.child = Some(child);
    s.started = true;
    s.busy = false;

    Ok("Sidecar started".into())
}

#[tauri::command]
async fn stop_sidecar(app: AppHandle, state: State<'_, SharedSidecarState>) -> Result<String, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.started = false;
    s.busy = false;
    s.request_tx.take();  // drop → writer task exits, stdin closes

    if let Some(tx) = s.response_tx.take() {
        let _ = tx.send(Err("Sidecar stopped".into()));
    }

    if let Some(mut child) = s.child.take() {
        // Try graceful kill, then force kill after 5 seconds
        let _ = child.kill();
        let _ = child.wait();
    }

    emit_sidecar_state(&app, false);
    Ok("Sidecar stopped".into())
}

#[tauri::command]
async fn rpc_call(
    app: AppHandle,
    state: State<'_, SharedSidecarState>,
    request: String,
) -> Result<Value, String> {
    let (tx, rx) = oneshot::channel();
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
        s.busy = true;
        s.response_tx = Some(tx);

        if req_tx.send(request).is_err() {
            s.busy = false;
            s.response_tx.take();
            return Err("Failed to send request to sidecar".into());
        }
    }

    emit_sidecar_state(&app, true);

    match tokio::time::timeout(std::time::Duration::from_secs(600), rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            let mut s = state.lock().map_err(|e| e.to_string())?;
            s.busy = false;
            emit_sidecar_state(&app, false);
            Err("Sidecar response channel closed".into())
        }
        Err(_) => {
            let mut s = state.lock().map_err(|e| e.to_string())?;
            s.busy = false;
            emit_sidecar_state(&app, false);
            Err("Sidecar operation timed out".into())
        }
    }
}

#[tauri::command]
async fn cancel_sidecar(state: State<'_, SharedSidecarState>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = &s.request_tx {
        let _ = tx.send(r#"{"type":"cancel"}"#.to_string());
    }
    Ok(())
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
    let mut stmt = db.conn
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
async fn create_patient(db: State<'_, Mutex<Database>>, data: CreatePatient) -> Result<Patient, String> {
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
    let affected = db.conn
        .execute("UPDATE patients SET name = ?1 WHERE id = ?2", params![data.name, data.id])
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("Patient not found".into());
    }
    Ok(())
}

#[tauri::command]
async fn delete_patient(db: State<'_, Mutex<Database>>, id: String) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM sessions WHERE patient_id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM patients WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
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
        (format!("SELECT {} FROM sessions WHERE patient_id = ?1 ORDER BY created_at DESC", SESSION_COLUMNS), true)
    } else {
        (format!("SELECT {} FROM sessions ORDER BY created_at DESC", SESSION_COLUMNS), false)
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
        s.inputs = fetch_session_inputs(&db.conn, &s.id);
        s.notes = fetch_session_notes(&db.conn, &s.id);
    }
    Ok(sessions)
}

#[tauri::command]
async fn create_session(db: State<'_, Mutex<Database>>, data: CreateSession) -> Result<Session, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let now = Local::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO sessions (id, patient_id, date, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, data.patient_id, data.date, now],
        )
        .map_err(|e| e.to_string())?;
    Ok(Session {
        id,
        patient_id: data.patient_id,
        date: data.date,
        created_at: now,
        inputs: Vec::new(),
        notes: Vec::new(),
    })
}

#[tauri::command]
async fn update_session(db: State<'_, Mutex<Database>>, data: UpdateSession) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let affected = db.conn
        .execute(
            "UPDATE sessions SET date = ?1 WHERE id = ?2",
            params![data.date, data.id],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("Session not found".into());
    }
    Ok(())
}

#[tauri::command]
async fn get_session(db: State<'_, Mutex<Database>>, id: String) -> Result<Option<Session>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let sql = format!("SELECT {} FROM sessions WHERE id = ?1", SESSION_COLUMNS);
    let mut stmt = db.conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map(params![id], map_session).map_err(|e| e.to_string())?;
    match rows.next() {
        Some(Ok(mut session)) => {
            session.inputs = fetch_session_inputs(&db.conn, &session.id);
            session.notes = fetch_session_notes(&db.conn, &session.id);
            Ok(Some(session))
        }
        Some(Err(e)) => Err(e.to_string()),
        None => Ok(None),
    }
}

#[tauri::command]
async fn delete_session(db: State<'_, Mutex<Database>>, id: String) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.conn
        .execute("DELETE FROM sessions WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
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
                language, transcription_model, include_in_notes, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![
                id,
                data.session_id,
                data.kind,
                data.source,
                data.title,
                data.text,
                data.audio_file,
                data.duration_seconds,
                data.language,
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
    let affected = db.conn
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
    get_session_input_by_id(&db.conn, &data.id)
}

#[tauri::command]
async fn delete_session_input(db: State<'_, Mutex<Database>>, id: String) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let affected = db
        .conn
        .execute("DELETE FROM session_inputs WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("Session input not found".into());
    }
    Ok(())
}

fn get_session_input_by_id(conn: &Connection, id: &str) -> Result<SessionInput, String> {
    conn.query_row(
        "SELECT id, session_id, kind, source, title, text, audio_file, duration_seconds, language, transcription_model, include_in_notes, created_at, updated_at
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
                language: row.get(8)?,
                transcription_model: row.get(9)?,
                include_in_notes: row.get::<_, i64>(10)? != 0,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
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
    db.conn.execute(
        "INSERT INTO session_notes (id, session_id, format, note, llm_model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(session_id, format) DO UPDATE SET note = ?4, llm_model = ?5",
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
async fn get_patient_formats(db: State<'_, Mutex<Database>>, patient_id: String) -> Result<Vec<String>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let key = format!("patient_formats_{}", patient_id);
    let mut stmt = db.conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    match rows.next() {
        Some(Ok(v)) => {
            let formats: Vec<String> = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            Ok(formats)
        }
        _ => Ok(Vec::new()),
    }
}

#[tauri::command]
async fn set_patient_formats(db: State<'_, Mutex<Database>>, patient_id: String, formats: Vec<String>) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let key = format!("patient_formats_{}", patient_id);
    let value = formats.join(",");
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
async fn get_setting(db: State<'_, Mutex<Database>>, key: String) -> Result<Option<String>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db.conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e.to_string()),
        None => Ok(None),
    }
}

#[tauri::command]
async fn set_setting(db: State<'_, Mutex<Database>>, key: String, value: String) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.conn
        .execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
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
async fn list_note_formats(db: State<'_, Mutex<Database>>) -> Result<Vec<NoteFormatTemplate>, String> {
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
async fn create_note_format(db: State<'_, Mutex<Database>>, data: CreateNoteFormat) -> Result<NoteFormatTemplate, String> {
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
async fn update_note_format(db: State<'_, Mutex<Database>>, data: UpdateNoteFormat) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // Get old name to cascade rename in session_notes
    let old_name: Option<String> = tx
        .query_row("SELECT name FROM note_formats WHERE id = ?1", params![data.id], |row| row.get(0))
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
    let is_builtin: i64 = db.conn
        .query_row("SELECT is_builtin FROM note_formats WHERE id = ?1", params![id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if is_builtin != 0 {
        return Err("Built-in formats cannot be deleted. Use Reset or Hide instead.".into());
    }
    // Refuse to delete if session_notes reference this format
    let name: String = db.conn
        .query_row("SELECT name FROM note_formats WHERE id = ?1", params![id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let note_count: i64 = db.conn
        .query_row("SELECT COUNT(*) FROM session_notes WHERE format = ?1", params![name], |row| row.get(0))
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
    let name: String = db.conn
        .query_row("SELECT name FROM note_formats WHERE id = ?1", params![id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let default_prompt = default_formats()
        .into_iter()
        .find(|(default_name, _)| default_name == &name)
        .map(|(_, prompt)| prompt);
    match default_prompt {
        Some(prompt) => {
            db.conn
                .execute("UPDATE note_formats SET prompt = ?1 WHERE id = ?2", params![prompt, id])
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        None => Err(format!("No default prompt for format '{}'", name)),
    }
}

#[tauri::command]
async fn toggle_note_format_hidden(db: State<'_, Mutex<Database>>, id: String) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.conn
        .execute("UPDATE note_formats SET hidden = NOT hidden WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── File Dialog ───────────────────────────────────────────────────────────

#[tauri::command]
async fn pick_audio_file(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Audio", &["wav", "mp3", "m4a", "flac", "ogg", "aiff", "aac"])
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

#[tauri::command]
async fn list_audio_devices() -> Result<Vec<audio::AudioDeviceInfo>, String> {
    audio::list_audio_devices().map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_recording(
    app: AppHandle,
    mic_device: Option<String>,
    system_device: Option<String>,
) -> Result<(), String> {
    audio::recorder::start_recording(app, mic_device, system_device).map_err(|e| e.to_string())
}

#[tauri::command]
async fn stop_recording(app: AppHandle) -> Result<audio::recorder::StopRecordingResult, String> {
    audio::recorder::stop_recording(app).map_err(|e| e.to_string())
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
        .setup(|app| {
            let handle = app.handle().clone();
            let db = Database::new(&handle)
                .map_err(|e| {
                    eprintln!("Failed to initialize database: {}", e);
                    e
                })?;
            app.manage(Mutex::new(db));
            app.manage(Arc::new(Mutex::new(SidecarState {
                request_tx: None,
                response_tx: None,
                child: None,
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
            pick_audio_file,
            list_note_formats,
            create_note_format,
            update_note_format,
            delete_note_format,
            reset_note_format,
            toggle_note_format_hidden,
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

use chrono::Local;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

// ── Database ──────────────────────────────────────────────────────────────

const SESSION_COLUMNS: &str = "id, patient_id, date, audio_file, duration_seconds, transcript, language, note, note_format, llm_model, transcription_model, created_at";

fn map_session(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        patient_id: row.get(1)?,
        date: row.get(2)?,
        audio_file: row.get(3)?,
        duration_seconds: row.get(4)?,
        transcript: row.get(5)?,
        language: row.get(6)?,
        note: row.get(7)?,
        note_format: row.get(8)?,
        llm_model: row.get(9)?,
        transcription_model: row.get(10)?,
        created_at: row.get(11)?,
    })
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
                audio_file TEXT,
                duration_seconds REAL,
                transcript TEXT,
                language TEXT,
                note TEXT,
                note_format TEXT,
                llm_model TEXT,
                transcription_model TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .map_err(|e| e.to_string())?;
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
    audio_file: Option<String>,
    duration_seconds: Option<f64>,
    transcript: Option<String>,
    language: Option<String>,
    note: Option<String>,
    note_format: Option<String>,
    llm_model: Option<String>,
    transcription_model: Option<String>,
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
    audio_file: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateSession {
    id: String,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    note_format: Option<String>,
    #[serde(default)]
    duration_seconds: Option<f64>,
    #[serde(default)]
    llm_model: Option<String>,
    #[serde(default)]
    transcription_model: Option<String>,
}

// ── Sidecar ───────────────────────────────────────────────────────────────

struct SidecarHandles {
    stdin: ChildStdin,
    stdout: ChildStdout,
    child: Child,
}

struct SidecarState {
    handles: Option<SidecarHandles>,
    started: bool,
}

// ── Sidecar Commands ──────────────────────────────────────────────────────

#[tauri::command]
async fn start_sidecar(app: AppHandle, state: State<'_, Mutex<SidecarState>>) -> Result<String, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    if s.handles.is_some() {
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
    // The metallib is loaded relative to the dylib location
    let mlx_lib_dir = sidecar_path.parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("_internal/mlx/lib");
    let dyld_path = std::env::var("DYLD_FALLBACK_LIBRARY_PATH").unwrap_or_default();
    let dyld_path = if dyld_path.is_empty() {
        mlx_lib_dir.to_string_lossy().into_owned()
    } else {
        format!("{}:{}", mlx_lib_dir.display(), dyld_path)
    };

    // Redirect stderr to a log file for debugging
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

    s.handles = Some(SidecarHandles {
        stdin,
        stdout,
        child,
    });
    s.started = true;

    Ok("Sidecar started".into())
}

#[tauri::command]
async fn stop_sidecar(state: State<'_, Mutex<SidecarState>>) -> Result<String, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    if let Some(handles) = s.handles.take() {
        s.started = false;
        let mut stdin = handles.stdin;
        let mut child = handles.child;
        let _ = writeln!(stdin, r#"{{"type":"exit"}}"#);
        let _ = stdin.flush();

        // Try graceful shutdown, then force kill after 5 seconds
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() > Duration::from_secs(5) {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => {
                    let _ = child.kill();
                    break;
                }
            }
        }
        Ok("Sidecar stopped".into())
    } else {
        Err("No sidecar running".into())
    }
}

#[tauri::command]
async fn rpc_call(
    app: AppHandle,
    state: State<'_, Mutex<SidecarState>>,
    request: String,
) -> Result<Value, String> {
    let handles = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.handles.take().ok_or("Sidecar not running")?
    };

    let app_clone = app.clone();
    let (result, returned_handles) = tokio::task::spawn_blocking(move || {
        let mut stdin = handles.stdin;
        let mut stdout = handles.stdout;
        let mut child = handles.child;

        if let Err(e) = writeln!(stdin, "{}", request) {
            return (Err(format!("Write error: {}", e)), None);
        }
        if let Err(e) = stdin.flush() {
            return (Err(format!("Flush error: {}", e)), None);
        }

        let reader = BufReader::new(&mut stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    // stdout read error — sidecar likely died. Kill + reap child.
                    let _ = child.kill();
                    let _ = child.wait();
                    return (Err(format!("Read error: {}", e)), None);
                }
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Skip non-JSON lines (stray prints) instead of failing
            let parsed: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };

            match parsed.get("type").and_then(|v| v.as_str()) {
                Some("progress") => {
                    let _ = app_clone.emit("sidecar-progress", &parsed);
                }
                Some("result") | Some("pong") => {
                    let handles = Some(SidecarHandles { stdin, stdout, child });
                    return (Ok(parsed), handles);
                }
                Some("error") => {
                    let msg = parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error");
                    // Sidecar is still alive after an error — restore handles
                    let handles = Some(SidecarHandles { stdin, stdout, child });
                    return (Err(msg.into()), handles);
                }
                _ => {}
            }
        }

        // EOF on stdout — sidecar died. Kill + reap child to avoid zombie.
        let _ = child.kill();
        let _ = child.wait();
        (Err("Sidecar closed connection unexpectedly".into()), None)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;

    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if let Some(h) = returned_handles {
            s.handles = Some(h);
        } else {
            // Sidecar died — clear started flag
            s.started = false;
        }
    }

    result
}

#[tauri::command]
async fn is_running(state: State<'_, Mutex<SidecarState>>) -> Result<bool, String> {
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
        (format!("SELECT {} FROM sessions WHERE patient_id = ?1 ORDER BY date DESC", SESSION_COLUMNS), true)
    } else {
        (format!("SELECT {} FROM sessions ORDER BY date DESC", SESSION_COLUMNS), false)
    };
    let mut stmt = db.conn.prepare(&sql).map_err(|e| e.to_string())?;
    let sessions: Vec<Session> = if has_param {
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
    Ok(sessions)
}

#[tauri::command]
async fn create_session(db: State<'_, Mutex<Database>>, data: CreateSession) -> Result<Session, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let now = Local::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO sessions (id, patient_id, date, audio_file, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, data.patient_id, data.date, data.audio_file, now],
        )
        .map_err(|e| e.to_string())?;
    Ok(Session {
        id,
        patient_id: data.patient_id,
        date: data.date,
        audio_file: data.audio_file,
        duration_seconds: None,
        transcript: None,
        language: None,
        note: None,
        note_format: None,
        llm_model: None,
        transcription_model: None,
        created_at: now,
    })
}

#[tauri::command]
async fn update_session(db: State<'_, Mutex<Database>>, data: UpdateSession) -> Result<(), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let affected = db.conn
        .execute(
            "UPDATE sessions SET
                transcript = COALESCE(?1, transcript),
                language = COALESCE(?2, language),
                note = COALESCE(?3, note),
                note_format = COALESCE(?4, note_format),
                duration_seconds = COALESCE(?5, duration_seconds),
                llm_model = COALESCE(?6, llm_model),
                transcription_model = COALESCE(?7, transcription_model)
             WHERE id = ?8",
            params![
                data.transcript,
                data.language,
                data.note,
                data.note_format,
                data.duration_seconds,
                data.llm_model,
                data.transcription_model,
                data.id
            ],
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
        Some(Ok(session)) => Ok(Some(session)),
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
            app.manage(Mutex::new(SidecarState { handles: None, started: false }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_sidecar,
            stop_sidecar,
            rpc_call,
            is_running,
            list_patients,
            create_patient,
            update_patient,
            delete_patient,
            list_sessions,
            create_session,
            update_session,
            get_session,
            delete_session,
            get_setting,
            set_setting,
            pick_audio_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

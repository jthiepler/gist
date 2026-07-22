import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { DEFAULT_DIARIZATION_SPEAKERS } from "./diarization";
import type {
  GenerateNotesResult,
  DiagnosticExportResult,
  DataExportResult,
  ModelsResult,
  NoteFormatTemplate,
  NoteGenerationFormat,
  NoteGenerationSource,
  RecordingJob,
  RestoreResult,
  Session,
  SessionInput,
  SessionNote,
  SidecarProgress,
} from "./types";

export async function startSidecar(): Promise<string> {
  return invoke<string>("start_sidecar");
}

export async function stopSidecar(): Promise<string> {
  return invoke<string>("stop_sidecar");
}

export async function isRunning(): Promise<boolean> {
  return invoke<boolean>("is_running");
}

export async function cancelSidecar(): Promise<void> {
  await invoke("cancel_sidecar");
}

async function rpcCall<T = unknown>(request: unknown): Promise<T> {
  return invoke<T>("rpc_call", { request: JSON.stringify(request) });
}

let developerFeaturesPromise: Promise<boolean> | null = null;

export function developerFeaturesEnabled(): Promise<boolean> {
  developerFeaturesPromise ??= invoke<boolean>("developer_features_enabled");
  return developerFeaturesPromise;
}

export async function onProgress(callback: (data: SidecarProgress) => void): Promise<UnlistenFn> {
  return listen<SidecarProgress>("sidecar-progress", (event) => {
    callback(event.payload);
  });
}

export async function listModels(): Promise<ModelsResult> {
  const result = await rpcCall<ModelsResult & { type: string }>({ type: "list_models" });
  return { llm: result.llm };
}

export interface TranscriptionSegment {
  start: number;
  end: number;
  text: string;
  speaker: string | null;
}

export interface TranscriptionResult {
  transcript: string;
  duration: number;
  segments: TranscriptionSegment[];
}

export async function transcribe(
  audioFile: string,
  diarize = false,
  numSpeakers: number = DEFAULT_DIARIZATION_SPEAKERS,
  model?: string,
): Promise<TranscriptionResult> {
  return rpcCall({
    type: "transcribe",
    audio_file: audioFile,
    diarize,
    num_speakers: numSpeakers,
    model: diarize ? model || undefined : undefined,
  });
}

export async function generateNote(
  sourceMaterial: string,
  format: string,
  model?: string,
  thinking?: boolean,
  prompt?: string,
): Promise<{ note: string; format: string }> {
  return rpcCall({
    type: "generate_note",
    transcript: sourceMaterial,
    format,
    model: model || undefined,
    thinking: thinking ?? false,
    prompt: prompt || undefined,
  });
}

export async function generateNotes(
  sources: NoteGenerationSource[],
  formats: NoteGenerationFormat[],
  model?: string,
  thinking?: boolean,
  verificationMode: "off" | "shadow" | "enforce" = "off",
  diagnostics?: { capture: boolean; sessionId: string },
): Promise<GenerateNotesResult> {
  return rpcCall({
    type: "generate_notes",
    sources,
    formats,
    model: model || undefined,
    thinking: thinking ?? false,
    verification_mode: verificationMode,
    capture_diagnostics: diagnostics?.capture ?? false,
    diagnostic_session_id: diagnostics?.capture ? diagnostics.sessionId : undefined,
  });
}

export async function exportSessionDiagnostics(
  sessionId: string,
): Promise<DiagnosticExportResult | null> {
  return invoke<DiagnosticExportResult | null>("export_session_diagnostics", { sessionId });
}

export async function exportBackup(passphrase: string | null): Promise<DataExportResult | null> {
  return invoke<DataExportResult | null>("export_backup", { passphrase });
}

export async function exportHumanArchive(passphrase: string | null): Promise<DataExportResult | null> {
  return invoke<DataExportResult | null>("export_human_archive", { passphrase });
}

export async function pickBackupForRestore(passphrase: string | null): Promise<DataExportResult | null> {
  return invoke<DataExportResult | null>("pick_backup_for_restore", { passphrase });
}

export async function restoreBackup(path: string, passphrase: string | null): Promise<RestoreResult> {
  return invoke<RestoreResult>("restore_backup", { path, passphrase });
}

export async function downloadModel(model: string): Promise<void> {
  await rpcCall({ type: "download_model", model, kind: "llm" });
}

export async function deleteModel(model: string): Promise<void> {
  await rpcCall({ type: "delete_model", model, kind: "llm" });
}

// ── Settings ───────────────────────────────────────────────────────────────

export async function getSetting(key: string): Promise<string | null> {
  return invoke<string | null>("get_setting", { key });
}

export async function setSetting(key: string, value: string): Promise<void> {
  return invoke<void>("set_setting", { key, value });
}

export async function setMenuBarEnabled(enabled: boolean): Promise<void> {
  return invoke<void>("set_menu_bar_enabled", { enabled });
}

export async function getSystemMemoryBytes(): Promise<number> {
  return invoke<number>("get_system_memory_bytes");
}

// ── Note Format Templates ─────────────────────────────────────────────────

export async function getPatientFormats(patientId: string): Promise<string[] | null> {
  return invoke<string[] | null>("get_patient_formats", { patientId });
}

export async function setPatientFormats(patientId: string, formats: string[]): Promise<void> {
  return invoke<void>("set_patient_formats", { patientId, formats });
}

export async function createSessionNote(
  sessionId: string,
  format: string,
  note: string,
  llmModel: string | null,
): Promise<SessionNote> {
  return invoke<SessionNote>("create_session_note", { sessionId, format, note, llmModel });
}

export async function createSessionInput(data: {
  session_id: string;
  kind: string;
  source: string;
  title: string;
  text: string;
  audio_file?: string | null;
  duration_seconds?: number | null;
  transcription_model?: string | null;
  include_in_notes?: boolean;
}): Promise<SessionInput> {
  return invoke<SessionInput>("create_session_input", { data });
}

export async function updateSessionInput(data: {
  id: string;
  title?: string;
  text?: string;
  include_in_notes?: boolean;
}): Promise<SessionInput> {
  return invoke<SessionInput>("update_session_input", { data });
}

export async function deleteSessionInput(id: string): Promise<void> {
  return invoke<void>("delete_session_input", { id });
}

export async function updateSession(data: {
  id: string;
  date: string;
  start_time?: string | null;
  title?: string | null;
  session_type?: string | null;
}): Promise<Session> {
  await invoke<void>("update_session", { data });
  const session = await getSession(data.id);
  if (!session) throw new Error("Session not found after update");
  return session;
}

export async function getSession(id: string): Promise<Session | null> {
  return invoke<Session | null>("get_session", { id });
}

export async function listNoteFormats(): Promise<NoteFormatTemplate[]> {
  return invoke<NoteFormatTemplate[]>("list_note_formats");
}

export async function createNoteFormat(name: string, prompt: string): Promise<NoteFormatTemplate> {
  return invoke<NoteFormatTemplate>("create_note_format", { data: { name, prompt } });
}

export async function updateNoteFormat(id: string, name: string, prompt: string): Promise<void> {
  return invoke<void>("update_note_format", { data: { id, name, prompt } });
}

export async function deleteNoteFormat(id: string): Promise<void> {
  return invoke<void>("delete_note_format", { id });
}

export async function resetNoteFormat(id: string): Promise<void> {
  return invoke<void>("reset_note_format", { id });
}

export async function toggleNoteFormatHidden(id: string): Promise<void> {
  return invoke<void>("toggle_note_format_hidden", { id });
}

// ── Audio Recording ─────────────────────────────────────────────────────────

export interface AudioDevice {
  id: string;
  name: string;
  device_type: string; // "input" | "output"
}

export interface RecordingStateInfo {
  is_recording: boolean;
  is_paused: boolean;
  elapsed_seconds: number;
  has_recording: boolean;
  file_path: string | null;
  job_id: string | null;
}

export interface StopRecordingResult {
  job_id: string;
  file_path: string;
  duration_seconds: number;
  is_short_recording: boolean;
}

export interface RecordingTickPayload {
  elapsed_seconds: number;
  level: number;
  is_paused: boolean;
}

export interface RecordingErrorPayload {
  message: string;
}

export interface RecordingStoppedPayload {
  job_id: string;
  file_path: string;
  duration_seconds: number;
  is_short_recording: boolean;
}

export async function listAudioDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_audio_devices");
}

export interface StartRecordingData {
  session_id: string;
  input_kind: string;
  formats: string[];
  llm_model: string;
  thinking: boolean;
  num_speakers: number;
  created_session: boolean;
}

export async function startRecording(
  data: StartRecordingData,
  micDevice?: string,
  systemDevice?: string,
): Promise<RecordingJob> {
  return invoke<RecordingJob>("start_recording", {
    data,
    micDevice: micDevice || null,
    systemDevice: systemDevice || null,
  });
}

export async function stopRecording(): Promise<StopRecordingResult> {
  return invoke<StopRecordingResult>("stop_recording");
}

export async function pauseRecording(): Promise<void> {
  await invoke<void>("pause_recording");
}

export async function resumeRecording(): Promise<void> {
  await invoke<void>("resume_recording");
}

export async function checkIsRecording(): Promise<boolean> {
  return invoke<boolean>("is_recording");
}

export async function getRecordingState(): Promise<RecordingStateInfo> {
  return invoke<RecordingStateInfo>("get_recording_state");
}

export async function listRecoverableRecordingJobs(): Promise<RecordingJob[]> {
  return invoke<RecordingJob[]>("list_recoverable_recording_jobs");
}

export async function getRecordingJob(id: string): Promise<RecordingJob> {
  return invoke<RecordingJob>("get_recording_job", { id });
}

export async function completeRecordingJob(id: string): Promise<void> {
  await invoke("complete_recording_job", { id });
}

export async function setRecordingJobError(id: string, error: string): Promise<void> {
  await invoke("set_recording_job_error", { id, error });
}

export async function discardRecordingJob(id: string): Promise<void> {
  await invoke("discard_recording_job", { id });
}

export async function onRecordingTick(callback: (data: RecordingTickPayload) => void): Promise<UnlistenFn> {
  return listen<RecordingTickPayload>("recording-tick", (event) => callback(event.payload));
}

export async function onRecordingStopped(callback: (data: RecordingStoppedPayload) => void): Promise<UnlistenFn> {
  return listen<RecordingStoppedPayload>("recording-stopped", (event) => callback(event.payload));
}

export async function onRecordingError(callback: (data: RecordingErrorPayload) => void): Promise<UnlistenFn> {
  return listen<RecordingErrorPayload>("recording-error", (event) => callback(event.payload));
}

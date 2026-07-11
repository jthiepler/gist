import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SidecarProgress, ModelsResult, NoteFormatTemplate, Session, SessionInput, SessionNote } from "./types";

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
  language: string;
  duration: number;
  segments: TranscriptionSegment[];
}

export async function transcribe(
  audioFile: string,
  diarize = false,
): Promise<TranscriptionResult> {
  return rpcCall({
    type: "transcribe",
    audio_file: audioFile,
    diarize,
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
    thinking: thinking ?? true,
    prompt: prompt || undefined,
  });
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

// ── Note Format Templates ─────────────────────────────────────────────────

export async function getPatientFormats(patientId: string): Promise<string[]> {
  return invoke<string[]>("get_patient_formats", { patientId });
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
  language?: string | null;
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
}): Promise<void> {
  return invoke<void>("update_session", { data });
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
  name: string;
  device_type: string; // "input" | "output"
}

export interface RecordingStateInfo {
  is_recording: boolean;
  is_paused: boolean;
  elapsed_seconds: number;
  has_recording: boolean;
  file_path: string | null;
}

export interface StopRecordingResult {
  file_path: string;
  duration_seconds: number;
}

export interface RecordingTickPayload {
  elapsed_seconds: number;
  level: number;
  is_paused: boolean;
}

export interface RecordingStoppedPayload {
  file_path: string;
  duration_seconds: number;
}

export async function listAudioDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_audio_devices");
}

export async function startRecording(micDevice?: string, systemDevice?: string): Promise<void> {
  await invoke<void>("start_recording", {
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

export async function onRecordingTick(callback: (data: RecordingTickPayload) => void): Promise<UnlistenFn> {
  return listen<RecordingTickPayload>("recording-tick", (event) => callback(event.payload));
}

export async function onRecordingStopped(callback: (data: RecordingStoppedPayload) => void): Promise<UnlistenFn> {
  return listen<RecordingStoppedPayload>("recording-stopped", (event) => callback(event.payload));
}

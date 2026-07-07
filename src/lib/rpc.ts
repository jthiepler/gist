import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SidecarProgress, ModelsResult, NoteFormatTemplate, SessionNote } from "./types";

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
  return { llm: result.llm, transcription: result.transcription };
}

export async function transcribe(
  audioFile: string,
  model?: string,
): Promise<{ transcript: string; language: string; duration: number; segments: unknown[] }> {
  return rpcCall({
    type: "transcribe",
    audio_file: audioFile,
    model: model || undefined,
  });
}

export async function generateNote(
  transcript: string,
  format: string,
  model?: string,
  thinking?: boolean,
  prompt?: string,
): Promise<{ note: string; format: string }> {
  return rpcCall({
    type: "generate_note",
    transcript,
    format,
    model: model || undefined,
    thinking: thinking ?? true,
    prompt: prompt || undefined,
  });
}

export async function downloadModel(model: string, kind: string): Promise<void> {
  await rpcCall({ type: "download_model", model, kind });
}

export async function deleteModel(model: string, kind: string): Promise<void> {
  await rpcCall({ type: "delete_model", model, kind });
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

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SidecarProgress, ModelsResult, NoteFormat } from "./types";

export async function startSidecar(): Promise<string> {
  return invoke<string>("start_sidecar");
}

export async function stopSidecar(): Promise<string> {
  return invoke<string>("stop_sidecar");
}

export async function isRunning(): Promise<boolean> {
  return invoke<boolean>("is_running");
}

export async function rpcCall<T = unknown>(request: unknown): Promise<T> {
  return invoke<T>("rpc_call", { request: JSON.stringify(request) });
}

export async function onProgress(callback: (data: SidecarProgress) => void): Promise<UnlistenFn> {
  return listen<SidecarProgress>("sidecar-progress", (event) => {
    callback(event.payload);
  });
}

export async function ping(): Promise<boolean> {
  try {
    await rpcCall({ type: "ping" });
    return true;
  } catch {
    return false;
  }
}

export async function listModels(): Promise<ModelsResult> {
  const result = await rpcCall<ModelsResult & { type: string }>({ type: "list_models" });
  return { llm: result.llm, transcription: result.transcription };
}

export async function listFormats(): Promise<NoteFormat[]> {
  const result = await rpcCall<{ formats: NoteFormat[] }>({ type: "list_formats" });
  return result.formats;
}

export async function transcribe(
  audioFile: string,
  model?: string,
  language?: string
): Promise<{ transcript: string; language: string; duration: number; segments: unknown[] }> {
  return rpcCall({
    type: "transcribe",
    audio_file: audioFile,
    model: model || undefined,
    language: language || undefined,
  });
}

export async function generateNote(
  transcript: string,
  format: string,
  model?: string,
  thinking?: boolean,
  language?: string
): Promise<{ note: string; format: string }> {
  return rpcCall({
    type: "generate_note",
    transcript,
    format,
    model: model || undefined,
    thinking: thinking ?? true,
    language: language || undefined,
  });
}

export async function downloadModel(model: string, kind: string): Promise<void> {
  await rpcCall({ type: "download_model", model, kind });
}

// ── Settings ───────────────────────────────────────────────────────────────

export async function getSetting(key: string): Promise<string | null> {
  return invoke<string | null>("get_setting", { key });
}

export async function setSetting(key: string, value: string): Promise<void> {
  await invoke<void>("set_setting", { key, value });
}

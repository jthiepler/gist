import { invoke } from "@tauri-apps/api/core";
import { transcribe, generateNote, createSessionNote, listNoteFormats } from "./rpc";
import { progressPercent, progressStage, progressBase, progressScale, currentOperation, activeOperation, sessionUpdate } from "./stores";
import { ensureSidecar } from "./ensureSidecar";
import type { Session, NoteFormatTemplate } from "./types";

export interface RecordingContext {
  patientId: string;
  formats: string[];
  defaultLlm: string;
  defaultTranscription: string;
  thinking: boolean;
}

export async function processSessionFromAudio(
  audioPath: string,
  ctx: RecordingContext,
): Promise<Session> {
  const ok = await ensureSidecar();
  if (!ok) {
    throw new Error("Failed to start the processing engine.");
  }

  const opId = "new-session";
  const sortedFormats = [...ctx.formats].sort((a, b) => a.localeCompare(b));

  currentOperation.set(opId);
  progressBase.set(0);
  progressScale.set(100);
  progressPercent.set(0);
  progressStage.set("Transcribing...");
  activeOperation.set({ type: "transcribe", label: "Transcribing audio..." });

  let transcript = "";
  let duration: number | null = null;
  let language: string | null = null;

  try {
    const result = await transcribe(audioPath, ctx.defaultTranscription || undefined);
    transcript = result.transcript;
    duration = result.duration;
    language = result.language;
  } catch (e) {
    const msg = String(e);
    const userMsg = msg === "sidecar_busy"
      ? "Another operation is in progress. Please wait or cancel it first."
      : `Transcription failed: ${msg}`;
    activeOperation.set({ type: null, label: "" });
    currentOperation.set(null);
    progressBase.set(0);
    progressScale.set(100);
    throw new Error(userMsg);
  }

  let session: Session;
  try {
    session = await invoke<Session>("create_session", {
      data: {
        patient_id: ctx.patientId,
        date: new Date().toISOString().slice(0, 10),
        audio_file: audioPath,
      },
    });
    await invoke("update_session", {
      data: {
        id: session.id,
        transcript,
        language: language || null,
        duration_seconds: duration,
        transcription_model: ctx.defaultTranscription || null,
      },
    });
    session = {
      ...session,
      transcript,
      language: language || null,
      duration_seconds: duration,
      transcription_model: ctx.defaultTranscription || null,
    };
    // Live-update: session now has transcript
    sessionUpdate.set(session);
  } catch (e) {
    activeOperation.set({ type: null, label: "" });
    currentOperation.set(null);
    throw new Error(`Failed to save session: ${e}`);
  }

  let templates: NoteFormatTemplate[] = [];
  try {
    templates = await listNoteFormats();
  } catch {}

  const totalNotes = sortedFormats.length;
  const basePct = 30;
  const noteRange = 70;

  for (let i = 0; i < sortedFormats.length; i++) {
    const fmtName = sortedFormats[i];
    const label = `Generating ${fmtName.toUpperCase()} note (${i + 1}/${totalNotes})...`;
    progressStage.set(label);
    activeOperation.set({ type: "generate_note", label });
    const fmtBase = basePct + Math.round((i / totalNotes) * noteRange);
    const fmtScale = Math.round(noteRange / totalNotes);
    progressBase.set(fmtBase);
    progressScale.set(fmtScale);

    try {
      const tmpl = templates.find((t) => t.name === fmtName);
      const result = await generateNote(
        transcript,
        fmtName,
        ctx.defaultLlm || undefined,
        ctx.thinking,
        tmpl?.prompt,
      );
      const sn = await createSessionNote(session.id, fmtName, result.note, ctx.defaultLlm || null);
      session.notes = [...session.notes, sn];
      // Live-update: session now has one more note
      sessionUpdate.set(session);
    } catch {
      break;
    }
  }

  progressPercent.set(100);
  progressStage.set("");
  activeOperation.set({ type: null, label: "" });
  currentOperation.set(null);
  progressBase.set(0);
  progressScale.set(100);

  return session;
}

import {
  transcribe,
  generateNote,
  createSessionNote,
  createSessionInput,
  getSession,
  listNoteFormats,
} from "./rpc";
import {
  formatInputsForNoteGeneration,
  SESSION_INPUT_LABELS,
  SESSION_INPUT_SOURCES,
} from "./sessionInputs";
import {
  progressPercent,
  progressStage,
  progressBase,
  progressScale,
  currentOperation,
  activeOperation,
  sessionUpdate,
} from "./stores";
import { ensureSidecar } from "./ensureSidecar";
import type { Session, NoteFormatTemplate, SessionInputKind } from "./types";

export interface RecordingContext {
  patientId: string;
  formats: string[];
  defaultLlm: string;
  defaultTranscription: string;
  thinking: boolean;
  inputKind: SessionInputKind;
  session?: Session;
  isNewSession?: boolean;
  regenerateExisting?: boolean;
}

function resetProgress() {
  progressStage.set("");
  activeOperation.set({ type: null, label: "" });
  currentOperation.set(null);
  progressBase.set(0);
  progressScale.set(100);
}

async function loadTemplates(): Promise<NoteFormatTemplate[]> {
  try {
    return await listNoteFormats();
  } catch {
    return [];
  }
}

async function generateDocumentationForSession(
  session: Session,
  formats: string[],
  ctx: RecordingContext,
): Promise<Session> {
  const sortedFormats = [...formats].sort((a, b) => a.localeCompare(b));
  if (sortedFormats.length === 0) return session;

  const noteSourceMaterial = formatInputsForNoteGeneration(session);
  const templates = await loadTemplates();
  const totalNotes = sortedFormats.length;
  const basePct = 30;
  const noteRange = 70;
  let nextSession = session;

  for (let i = 0; i < sortedFormats.length; i++) {
    const fmtName = sortedFormats[i];
    const label = `${ctx.isNewSession ? "Generating" : "Updating"} ${fmtName.toUpperCase()} note (${i + 1}/${totalNotes})...`;
    progressStage.set(label);
    activeOperation.set({ type: "generate_note", label });
    progressBase.set(basePct + Math.round((i / totalNotes) * noteRange));
    progressScale.set(Math.round(noteRange / totalNotes));

    try {
      const tmpl = templates.find((t) => t.name === fmtName);
      const result = await generateNote(
        noteSourceMaterial,
        fmtName,
        ctx.defaultLlm || undefined,
        ctx.thinking,
        tmpl?.prompt,
      );
      const note = await createSessionNote(
        session.id,
        fmtName,
        result.note,
        ctx.defaultLlm || null,
      );
      nextSession = {
        ...nextSession,
        notes: [
          ...nextSession.notes.filter((existing) => existing.format !== note.format),
          note,
        ].sort((a, b) => a.format.localeCompare(b.format)),
      };
      sessionUpdate.set(nextSession);
    } catch {
      break;
    }
  }

  return nextSession;
}

export async function processSessionFromAudio(
  audioPath: string,
  ctx: RecordingContext,
): Promise<Session> {
  if (!ctx.session) {
    throw new Error("Recording session was not initialized.");
  }

  const session = ctx.session;
  const ok = await ensureSidecar();
  if (!ok) {
    throw new Error("Failed to start processing engine.");
  }

  const opId = ctx.isNewSession
    ? `new-session-${session.id}`
    : `add-input-${session.id}`;

  currentOperation.set(opId);
  progressBase.set(0);
  progressScale.set(100);
  progressPercent.set(0);

  const isClinicianDictation = ctx.inputKind === "clinician_note";
  const transcribingLabel = isClinicianDictation
    ? "Transcribing clinician note..."
    : "Transcribing session recording...";
  progressStage.set(transcribingLabel);
  activeOperation.set({ type: "transcribe", label: transcribingLabel });

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
    resetProgress();
    throw new Error(
      msg === "sidecar_busy"
        ? "Another operation is in progress. Please wait or cancel it first."
        : `Transcription failed: ${msg}`,
    );
  }

  const source = isClinicianDictation
    ? SESSION_INPUT_SOURCES.dictation
    : SESSION_INPUT_SOURCES.recording;

  try {
    progressStage.set("Saving session material...");
    activeOperation.set({ type: "create_session", label: "Saving session material..." });

    await createSessionInput({
      session_id: session.id,
      kind: ctx.inputKind,
      source,
      title: SESSION_INPUT_LABELS[ctx.inputKind],
      text: transcript,
      audio_file: audioPath,
      duration_seconds: duration,
      language,
      transcription_model: ctx.defaultTranscription || null,
      include_in_notes: true,
    });
    const updatedSession = (await getSession(session.id)) ?? session;
    sessionUpdate.set(updatedSession);
    if (!ctx.isNewSession && !ctx.regenerateExisting) {
      progressPercent.set(100);
      resetProgress();
      return updatedSession;
    }
    const formatsToRefresh = ctx.isNewSession
      ? ctx.formats
      : ctx.formats.length > 0
        ? ctx.formats
        : updatedSession.notes.map((note) => note.format);
    const processedSession = await generateDocumentationForSession(
      updatedSession,
      formatsToRefresh,
      ctx,
    );
    progressPercent.set(100);
    resetProgress();
    return processedSession;
  } catch (e) {
    resetProgress();
    throw new Error(`Failed to save session: ${String(e)}`);
  }
}

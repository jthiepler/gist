import {
  transcribe,
  createSessionInput,
  getSession,
  completeRecordingJob,
} from "./rpc";
import type { TranscriptionSegment } from "./rpc";
import {
  SESSION_INPUT_LABELS,
  SESSION_INPUT_SOURCES,
} from "./sessionInputs";
import { generateSessionDocumentation } from "./documentation";
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
import { DEFAULT_DIARIZATION_SPEAKERS } from "./diarization";
import type { Session, SessionInputKind } from "./types";

export interface RecordingContext {
  patientId: string;
  occurrenceDate?: string;
  startTime?: string;
  title?: string;
  sessionType?: string;
  formats: string[];
  defaultLlm: string;
  thinking: boolean;
  inputKind: SessionInputKind;
  diarize: boolean;
  numSpeakers: number;
  session?: Session;
  isNewSession?: boolean;
  regenerateExisting?: boolean;
  jobId?: string;
}

function resetProgress() {
  progressStage.set("");
  activeOperation.set({ type: null, label: "" });
  currentOperation.set(null);
  progressBase.set(0);
  progressScale.set(100);
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
  const source = isClinicianDictation
    ? SESSION_INPUT_SOURCES.dictation
    : SESSION_INPUT_SOURCES.recording;
  const existingRecordingInput = ctx.jobId
    ? session.inputs.find(
        (input) => input.audio_file === audioPath && input.source === source,
      )
    : undefined;
  const transcribingLabel = isClinicianDictation
    ? "Transcribing clinician note..."
    : "Transcribing session recording...";
  progressStage.set(transcribingLabel);
  activeOperation.set({ type: "transcribe", label: transcribingLabel });

  let transcript = existingRecordingInput?.text ?? "";
  let duration: number | null = existingRecordingInput?.duration_seconds ?? null;
  let segments: TranscriptionSegment[] | undefined;

  if (!existingRecordingInput) {
    try {
      const result = await transcribe(
        audioPath,
        ctx.inputKind === "session_transcript" && ctx.diarize,
        ctx.numSpeakers ?? DEFAULT_DIARIZATION_SPEAKERS,
        ctx.defaultLlm,
      );
      transcript = result.transcript;
      duration = result.duration;
      segments = result.segments;
    } catch (e) {
      const msg = String(e);
      resetProgress();
      throw new Error(
        msg === "sidecar_busy"
          ? "Another operation is in progress. Please wait or cancel it first."
          : `Transcription failed: ${msg}`,
      );
    }
  }

  let updatedSession: Session;
  try {
    progressStage.set("Saving source material...");
    activeOperation.set({ type: "create_session", label: "Saving source material..." });

    if (!existingRecordingInput) {
      await createSessionInput({
        session_id: session.id,
        kind: ctx.inputKind,
        source,
        title: SESSION_INPUT_LABELS[ctx.inputKind],
        text: transcript,
        audio_file: audioPath,
        duration_seconds: duration,
        metadata_json: segments ? JSON.stringify({ segments }) : null,
        include_in_notes: true,
      });
    }
    updatedSession = existingRecordingInput
      ? session
      : (await getSession(session.id)) ?? session;
    sessionUpdate.set(updatedSession);
  } catch (e) {
    resetProgress();
    throw new Error(`Failed to save session: ${String(e)}`);
  }

  if (!ctx.isNewSession && !ctx.regenerateExisting) {
    try {
      if (ctx.jobId) await completeRecordingJob(ctx.jobId);
    } catch (e) {
      resetProgress();
      throw new Error(`Source was saved, but recording recovery could not be completed: ${String(e)}`);
    }
    progressPercent.set(100);
    resetProgress();
    return updatedSession;
  }

  const formatsToRefresh = ctx.isNewSession
    ? ctx.formats
    : updatedSession.notes.map((note) => note.format);

  try {
    const processedSession = await generateSessionDocumentation(updatedSession, formatsToRefresh, {
      defaultLlm: ctx.defaultLlm,
      thinking: ctx.thinking,
      verb: ctx.isNewSession ? "Generating" : "Updating",
      onSessionUpdate: (nextSession) => sessionUpdate.set(nextSession),
    });
    if (ctx.jobId) await completeRecordingJob(ctx.jobId);
    progressPercent.set(100);
    resetProgress();
    return processedSession;
  } catch (e) {
    resetProgress();
    throw e;
  }
}

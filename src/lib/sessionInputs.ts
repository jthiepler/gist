import type { NoteGenerationSource, Session, SessionInput, SessionInputKind } from "./types";

export const SESSION_INPUT_LABELS: Record<SessionInputKind, string> = {
  session_transcript: "Session transcript",
  clinician_note: "Clinician note",
};

export const SESSION_INPUT_SOURCES = {
  typed: "typed",
  uploadAudio: "upload_audio",
  recording: "recording",
  dictation: "dictation",
} as const;

const INPUT_ORDER: Record<string, number> = {
  session_transcript: 0,
  clinician_note: 1,
};

export function getInputLabel(input: Pick<SessionInput, "kind" | "title">): string {
  if (input.title) return input.title;
  return SESSION_INPUT_LABELS[input.kind as SessionInputKind] ?? input.kind;
}

export function getInputsForNotes(session: Session): SessionInput[] {
  return session.inputs
    .filter((input) => input.include_in_notes && input.text.trim().length > 0)
    .sort((a, b) => {
      const orderA = INPUT_ORDER[a.kind] ?? 99;
      const orderB = INPUT_ORDER[b.kind] ?? 99;
      if (orderA !== orderB) return orderA - orderB;
      return a.created_at.localeCompare(b.created_at);
    });
}

export function formatInputsForNoteGeneration(session: Session): string {
  const inputs = getInputsForNotes(session);
  return inputs
    .map((input) => `## ${getInputLabel(input)}\n\n${input.text.trim()}`)
    .join("\n\n---\n\n");
}

export function getNoteGenerationSources(session: Session): NoteGenerationSource[] {
  return getInputsForNotes(session).map((input) => ({
    id: input.id,
    kind: input.kind,
    origin: input.source,
    title: getInputLabel(input),
    text: input.text.trim(),
  }));
}

export function hasNoteSourceMaterial(session: Session): boolean {
  return getInputsForNotes(session).length > 0;
}

export function getSessionDurationSeconds(session: Session): number | null {
  const durations = session.inputs
    .map((input) => input.duration_seconds)
    .filter((duration): duration is number => typeof duration === "number");
  if (durations.length === 0) return null;
  return durations.reduce((sum, duration) => sum + duration, 0);
}

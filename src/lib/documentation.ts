import { createSessionNote, generateNotes, listNoteFormats } from "./rpc";
import { getNoteGenerationSources } from "./sessionInputs";
import {
  activeOperation,
  evidenceModelRecoveryRequested,
  progressBase,
  progressScale,
  progressStage,
} from "./stores";
import type { NoteFormatTemplate, Session } from "./types";
import { loadSettings } from "./settings";
import { isMissingEvidenceModelError } from "./models";

export interface GenerateDocumentationOptions {
  defaultLlm: string;
  thinking: boolean;
  verb: "Creating" | "Generating" | "Updating";
  onSessionUpdate?: (session: Session) => void;
}

export async function generateSessionDocumentation(
  session: Session,
  formats: string[],
  options: GenerateDocumentationOptions,
): Promise<Session> {
  const selectedFormats = [...new Set(formats)].sort((a, b) => a.localeCompare(b));
  if (selectedFormats.length === 0) return session;

  const sources = getNoteGenerationSources(session);
  let templates: NoteFormatTemplate[] = [];
  try {
    templates = await listNoteFormats();
  } catch {
    // Built-in prompts remain available in the sidecar when templates cannot load.
  }

  let updatedSession = session;

  const label = selectedFormats.length === 1
    ? `${options.verb} ${selectedFormats[0].toUpperCase()} note...`
    : `${options.verb} ${selectedFormats.length} notes...`;
  progressStage.set(label);
  activeOperation.set({ type: "generate_note", label });
  progressBase.set(30);
  progressScale.set(70);

  const requestedFormats = selectedFormats.map((format) => {
    const template = templates.find((item) => item.name === format);
    return {
      name: format,
      prompt: template?.prompt,
    };
  });
  const settings = await loadSettings();
  let result: Awaited<ReturnType<typeof generateNotes>>;
  try {
    result = await generateNotes(
      sources,
      requestedFormats,
      options.defaultLlm || undefined,
      options.thinking,
      "off",
      settings.captureNoteDiagnostics
        ? { capture: true, sessionId: session.id }
        : undefined,
    );
  } catch (e) {
    if (isMissingEvidenceModelError(e)) {
      evidenceModelRecoveryRequested.set(true);
    }
    throw e;
  }

  for (const generated of result.notes) {
    const note = await createSessionNote(
      session.id,
      generated.format,
      generated.note,
      options.defaultLlm || null,
    );
    updatedSession = {
      ...updatedSession,
      notes: [
        ...updatedSession.notes.filter((existing) => existing.format !== note.format),
        note,
      ].sort((a, b) => a.format.localeCompare(b.format)),
    };
    options.onSessionUpdate?.(updatedSession);
  }

  if (result.failures.length > 0) {
    const details = result.failures
      .map((failure) => `${failure.format.toUpperCase()}: ${failure.message}`)
      .join("; ");
    throw new Error(details);
  }

  return updatedSession;
}

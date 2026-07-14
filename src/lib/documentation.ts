import { createSessionNote, generateNotes, listNoteFormats } from "./rpc";
import { getSourcesForNoteGeneration } from "./sessionInputs";
import { activeOperation, progressBase, progressScale, progressStage } from "./stores";
import type { NoteFormatTemplate, Session } from "./types";

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

  const sources = getSourcesForNoteGeneration(session);
  let templates: NoteFormatTemplate[] = [];
  try {
    templates = await listNoteFormats();
  } catch {
    // Built-in prompts remain available in the sidecar when templates cannot load.
  }

  const requestedFormats = selectedFormats.map((name) => {
    const template = templates.find((item) => item.name === name);
    return { name, ...(template?.prompt ? { prompt: template.prompt } : {}) };
  });
  let updatedSession = session;

  const label = `${options.verb} clinical notes...`;
  progressStage.set(label);
  activeOperation.set({ type: "generate_note", label });
  progressBase.set(30);
  progressScale.set(70);

  try {
    const result = await generateNotes(
      sources,
      requestedFormats,
      options.defaultLlm || undefined,
      options.thinking,
    );
    for (const format of selectedFormats) {
      const generated = result.notes[format];
      if (!generated) throw new Error(`No ${format.toUpperCase()} note was returned.`);
      const note = await createSessionNote(
        session.id,
        format,
        generated,
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
  } catch (error) {
    throw new Error(`Clinical note generation failed: ${String(error)}`);
  }

  return updatedSession;
}

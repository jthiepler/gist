import { createSessionNote, generateNote, listNoteFormats } from "./rpc";
import { formatInputsForNoteGeneration } from "./sessionInputs";
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

  const sourceMaterial = formatInputsForNoteGeneration(session);
  let templates: NoteFormatTemplate[] = [];
  try {
    templates = await listNoteFormats();
  } catch {
    // Built-in prompts remain available in the sidecar when templates cannot load.
  }

  const totalNotes = selectedFormats.length;
  const noteRange = 70;
  let updatedSession = session;

  for (let index = 0; index < totalNotes; index += 1) {
    const format = selectedFormats[index];
    const label = `${options.verb} ${format.toUpperCase()} note (${index + 1}/${totalNotes})...`;
    progressStage.set(label);
    activeOperation.set({ type: "generate_note", label });
    progressBase.set(30 + Math.round((index / totalNotes) * noteRange));
    progressScale.set(Math.round(noteRange / totalNotes));

    try {
      const template = templates.find((item) => item.name === format);
      const result = await generateNote(
        sourceMaterial,
        format,
        options.defaultLlm || undefined,
        options.thinking,
        template?.prompt,
      );
      const note = await createSessionNote(
        session.id,
        format,
        result.note,
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
    } catch (error) {
      throw new Error(`${format.toUpperCase()} note generation failed: ${String(error)}`);
    }
  }

  return updatedSession;
}

import { ask } from "@tauri-apps/plugin-dialog";

export function confirmRegenerateAttachedNotes(noteCount: number): Promise<boolean> {
  if (noteCount === 0) return Promise.resolve(false);

  return ask(
    `The current content of ${noteCount} attached ${noteCount === 1 ? "note" : "notes"} will be replaced with newly generated versions. Your source material will not be changed.`,
    {
      title: "Regenerate attached notes?",
      kind: "warning",
    }
  );
}

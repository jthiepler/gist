import { ask } from "@tauri-apps/plugin-dialog";

export function confirmRegenerateAttachedNotes(noteCount: number): Promise<boolean> {
  if (noteCount === 0) return Promise.resolve(false);

  return ask(
    "This will override the current content of all attached notes. Do you want to continue?",
    {
      title: "Regenerate attached notes?",
      kind: "warning",
    }
  );
}

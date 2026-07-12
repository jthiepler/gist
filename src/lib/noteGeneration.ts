export interface NoteGenerationSelection {
  regenerateExisting?: boolean;
  formats?: string[];
}

export function selectNoteFormats(
  existingFormats: string[],
  preferredFormats: string[],
  options: NoteGenerationSelection = {},
): string[] {
  const existing = new Set(existingFormats);
  const requested = options.formats
    ?? (options.regenerateExisting ? existingFormats : preferredFormats.length > 0 ? preferredFormats : ["soap"]);

  return [...new Set(requested)]
    .filter((format) => options.regenerateExisting || !existing.has(format))
    .sort((a, b) => a.localeCompare(b));
}

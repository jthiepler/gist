import type { RecordingContext } from "./processSession";

export function isNewSessionRecording(
  context: Pick<RecordingContext, "isNewSession"> | null | undefined,
): boolean {
  return context?.isNewSession === true;
}

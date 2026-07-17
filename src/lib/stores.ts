import { writable } from "svelte/store";
import type { Patient, Session } from "./types";
import type { RecordingContext } from "./processSession";

// Patient list — shared between sidebar and workspace
export const patients = writable<Patient[]>([]);

// Currently selected patient ID (derived from URL, but stored for sidebar highlight)
export const selectedPatientId = writable<string | null>(null);

// Sidecar — internal only, not shown in main UI
export const sidecarRunning = writable<boolean>(false);

// Progress — includes an operation ID so concurrent operations don't clobber
export const progressPercent = writable<number>(0);
export const progressStage = writable<string>("");
export const progressEta = writable<number | null>(null);
export const progressBase = writable<number>(0);
export const progressScale = writable<number>(100);
export const currentOperation = writable<string | null>(null);

// Sidecar busy state — true when an operation is in flight
export const sidecarBusy = writable<boolean>(false);
// Active operation descriptor for the persistent progress banner
export const activeOperation = writable<{ type: string | null; label: string }>({ type: null, label: "" });
// Opens the required-model recovery gate if the extractor disappears while
// the app is already running (for example, after external storage cleanup).
export const evidenceModelRecoveryRequested = writable<boolean>(false);

// Dark mode
export const darkMode = writable<boolean>(false);
export const appearance = writable<"system" | "light" | "dark">("system");

// Recording state
export const isRecording = writable<boolean>(false);
export const recordingPaused = writable<boolean>(false);
export const recordingElapsed = writable<number>(0);
export const recordingLevel = writable<number>(0);

// Recording context — saved when starting a recording so the layout can
// trigger transcription + note generation when recording stops, even if
// the NewSessionPanel component has been unmounted.
export const recordingContext = writable<RecordingContext | null>(null);

// Covers the durable-job handoff window before/after the sidecar itself is
// busy, so destructive actions and updater relaunches cannot race processing.
export const recordingJobsProcessing = writable<boolean>(false);

// Latest session snapshot produced by background processing. Keeping the latest
// value ensures Svelte cannot batch away the final update and lets a newly
// mounted patient page consume it as well.
export const sessionUpdate = writable<Session | null>(null);

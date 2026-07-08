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

// Dark mode
export const darkMode = writable<boolean>(false);

// Recording state
export const isRecording = writable<boolean>(false);
 export const recordingElapsed = writable<number>(0);
export const recordingLevel = writable<number>(0);

// Recording context — saved when starting a recording so the layout can
// trigger transcription + note generation when recording stops, even if
// the NewSessionPanel component has been unmounted.
export const recordingContext = writable<RecordingContext | null>(null);

// Pending session — set by the layout after processing a recording,
// consumed by the patient page to prepend to its session list.
export const pendingSession = writable<Session | null>(null);

// Live session update — fires at each step of processing (transcript saved,
// each note generated) so the patient page can update in real time.
export const sessionUpdate = writable<Session | null>(null);

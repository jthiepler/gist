import { writable } from "svelte/store";
import type { Patient } from "./types";

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
export const progressAudioDuration = writable<number | null>(null);
export const currentOperation = writable<string | null>(null);

// Dark mode
export const darkMode = writable<boolean>(false);

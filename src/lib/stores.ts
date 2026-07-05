import { writable } from "svelte/store";
import type { Patient } from "./types";

// Patient list — shared between sidebar and workspace
export const patients = writable<Patient[]>([]);

// Currently selected patient ID (derived from URL, but stored for sidebar highlight)
export const selectedPatientId = writable<string | null>(null);

// Sidecar — internal only, not shown in main UI
export const sidecarRunning = writable<boolean>(false);

// Transcription / generation progress
export const progressPercent = writable<number>(0);
export const progressStage = writable<string>("");

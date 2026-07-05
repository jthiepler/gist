import { writable } from "svelte/store";
import type { Patient, Session } from "./types";

export const currentPatient = writable<Patient | null>(null);
export const currentSession = writable<Session | null>(null);
export const sidecarRunning = writable<boolean>(false);
export const transcribing = writable<boolean>(false);
export const generating = writable<boolean>(false);

export const progressPercent = writable<number>(0);
export const progressStage = writable<string>("");

// Patient name lookup cache (avoids N+1 queries)
export const patientMap = writable<Record<string, Patient>>({});

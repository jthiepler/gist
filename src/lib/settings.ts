import { developerFeaturesEnabled, getSetting, setSetting } from "./rpc";
import { appearance } from "./stores";

export interface Settings {
  defaultLlm: string;
  appearance: "system" | "light" | "dark";
  confirmRecordingConsent: boolean;
  developerFeaturesEnabled: boolean;
  captureNoteDiagnostics: boolean;
}

const DEFAULTS: Settings = {
  defaultLlm: "",
  appearance: "system",
  confirmRecordingConsent: true,
  developerFeaturesEnabled: false,
  captureNoteDiagnostics: false,
};

export async function loadSettings(): Promise<Settings> {
  const s = { ...DEFAULTS };
  try {
    s.developerFeaturesEnabled = await developerFeaturesEnabled();
  } catch (e) {
    console.error("developerFeaturesEnabled failed:", e);
  }
  try {
    const llm = await getSetting("default_llm");
    if (llm) s.defaultLlm = llm;
    const ap = await getSetting("appearance");
    if (ap === "system" || ap === "light" || ap === "dark") s.appearance = ap;
    const consent = await getSetting("confirm_recording_consent");
    if (consent !== null) s.confirmRecordingConsent = consent === "true";
    if (s.developerFeaturesEnabled) {
      const diagnostics = await getSetting("capture_note_generation_diagnostics");
      if (diagnostics !== null) s.captureNoteDiagnostics = diagnostics === "true";
    }
  } catch (e) {
    console.error("loadSettings failed:", e);
  }
  return s;
}

export async function saveSetting(key: string, value: string): Promise<void> {
  await setSetting(key, value);
}

export async function loadAppearance(): Promise<void> {
  try {
    const ap = await getSetting("appearance");
    appearance.set(ap === "system" || ap === "light" || ap === "dark" ? ap : "system");
  } catch (e) {
    console.error("loadAppearance failed:", e);
  }
}

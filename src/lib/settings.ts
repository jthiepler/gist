import { getSetting, setSetting } from "./rpc";
import { appearance } from "./stores";

export interface Settings {
  defaultLlm: string;
  thinking: boolean;
  appearance: "system" | "light" | "dark";
  confirmRecordingConsent: boolean;
}

const DEFAULTS: Settings = {
  defaultLlm: "",
  thinking: false,
  appearance: "system",
  confirmRecordingConsent: true,
};

export async function loadSettings(): Promise<Settings> {
  const s = { ...DEFAULTS };
  try {
    const llm = await getSetting("default_llm");
    if (llm) s.defaultLlm = llm;
    const th = await getSetting("thinking");
    if (th !== null) s.thinking = th === "true";
    const ap = await getSetting("appearance");
    if (ap === "system" || ap === "light" || ap === "dark") s.appearance = ap;
    const consent = await getSetting("confirm_recording_consent");
    if (consent !== null) s.confirmRecordingConsent = consent === "true";
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

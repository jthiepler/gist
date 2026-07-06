import { getSetting, setSetting } from "./rpc";
import { darkMode } from "./stores";

export interface Settings {
  defaultFormat: string;
  defaultLlm: string;
  defaultTranscription: string;
  thinking: boolean;
  darkMode: boolean;
}

const DEFAULTS: Settings = {
  defaultFormat: "soap",
  defaultLlm: "",
  defaultTranscription: "",
  thinking: false,
  darkMode: false,
};

export async function loadSettings(): Promise<Settings> {
  const s = { ...DEFAULTS };
  try {
    const fmt = await getSetting("default_format");
    if (fmt) s.defaultFormat = fmt;
    const llm = await getSetting("default_llm");
    if (llm) s.defaultLlm = llm;
    const tr = await getSetting("default_transcription");
    if (tr) s.defaultTranscription = tr;
    const th = await getSetting("thinking");
    if (th !== null) s.thinking = th === "true";
    const dm = await getSetting("dark_mode");
    if (dm !== null) s.darkMode = dm === "true";
  } catch {}
  return s;
}

export async function saveSettings(s: Settings): Promise<void> {
  await setSetting("default_format", s.defaultFormat);
  await setSetting("default_llm", s.defaultLlm);
  await setSetting("default_transcription", s.defaultTranscription);
  await setSetting("thinking", s.thinking.toString());
  await setSetting("dark_mode", s.darkMode.toString());
}

export async function loadDarkMode(): Promise<boolean> {
  try {
    const dm = await getSetting("dark_mode");
    const val = dm === "true";
    darkMode.set(val);
    return val;
  } catch {
    return false;
  }
}

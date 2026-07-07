export interface Patient {
  id: string;
  name: string;
  created_at: string;
}

export interface Session {
  id: string;
  patient_id: string;
  date: string;
  audio_file: string | null;
  duration_seconds: number | null;
  transcript: string | null;
  language: string | null;
  note: string | null;
  note_format: string | null;
  llm_model: string | null;
  transcription_model: string | null;
  created_at: string;
  notes: SessionNote[];
}

export interface SessionNote {
  id: string;
  session_id: string;
  format: string;
  note: string | null;
  llm_model: string | null;
  created_at: string;
}

export interface SidecarProgress {
  type: "progress";
  percent: number;
  stage: string;
  eta_seconds?: number;
  audio_duration?: number;
}

export interface ModelInfo {
  display: string;
  backend: string;
  size_gb: number;
  description: string;
  downloaded: boolean;
}

export interface ModelsResult {
  llm: Record<string, ModelInfo>;
  transcription: Record<string, ModelInfo>;
}

export interface NoteFormatTemplate {
  id: string;
  name: string;
  prompt: string;
  is_builtin: boolean;
  hidden: boolean;
  created_at: string;
}

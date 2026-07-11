export interface Patient {
  id: string;
  name: string;
  created_at: string;
}

export interface Session {
  id: string;
  patient_id: string;
  date: string;
  start_time: string | null;
  title: string | null;
  session_type: string | null;
  updated_at: string | null;
  created_at: string;
  inputs: SessionInput[];
  notes: SessionNote[];
}

export type SessionInputKind = "session_transcript" | "clinician_note";

export interface SessionInput {
  id: string;
  session_id: string;
  kind: SessionInputKind | string;
  source: string;
  title: string;
  text: string;
  audio_file: string | null;
  duration_seconds: number | null;
  language: string | null;
  transcription_model: string | null;
  include_in_notes: boolean;
  created_at: string;
  updated_at: string;
}

export interface SessionNote {
  id: string;
  session_id: string;
  format: string;
  note: string | null;
  llm_model: string | null;
  created_at: string;
}

export interface RecordingJob {
  id: string;
  session_id: string;
  audio_file: string;
  input_kind: SessionInputKind | string;
  formats: string[];
  llm_model: string;
  thinking: boolean;
  diarize: boolean;
  created_session: boolean;
  state: "recording" | "recorded" | "failed" | "completed" | string;
  error: string | null;
  created_at: string;
  updated_at: string;
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
}

export interface NoteFormatTemplate {
  id: string;
  name: string;
  prompt: string;
  is_builtin: boolean;
  hidden: boolean;
  created_at: string;
}

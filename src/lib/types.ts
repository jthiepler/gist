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
  updated_at: string | null;
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
  num_speakers: number;
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
  // null means the sidecar has not confirmed the local cache state yet.
  downloaded: boolean | null;
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

export interface NoteGenerationSource {
  id: string;
  kind: string;
  origin: string;
  title: string;
  text: string;
}

export interface NoteGenerationFormat {
  name: string;
  prompt?: string;
}

export interface NoteVerificationSummary {
  claims_checked: number;
  supported: number;
  partly_supported: number;
  unsupported: number;
  contradicted: number;
  claims_removed: number;
}

export interface GeneratedNoteResult {
  format: string;
  note: string;
  verification: NoteVerificationSummary;
}

export interface NoteGenerationFailure {
  format: string;
  message: string;
}

export interface GenerateNotesResult {
  notes: GeneratedNoteResult[];
  failures: NoteGenerationFailure[];
  ledger_stats: {
    documents: number;
    units: number;
    blocks: number;
    evidence_records: number;
    retry_count: number;
    evidence_tokens: number;
  };
}

export interface DiagnosticExportResult {
  path: string;
  run_count: number;
}

export interface RecordCounts {
  patient_count: number;
  session_count: number;
}

export interface DataExportResult extends RecordCounts {
  path: string;
}

export type RestoreResult = RecordCounts;

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { transcribe, generateNote, listFormats } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import { progressPercent, progressStage, progressEta, currentOperation } from "$lib/stores";
  import { loadSettings } from "$lib/settings";
  import type { Session, NoteFormat } from "$lib/types";

  let {
    patientId,
    onComplete,
    onCancel,
  }: {
    patientId: string;
    onComplete: (session: Session) => void;
    onCancel: () => void;
  } = $props();

  let audioPath = $state("");
  let selectedFormat = $state("soap");
  let formats = $state<NoteFormat[]>([]);
  let formatsLoaded = $state(false);
  let error = $state("");
  let phase = $state<"idle" | "transcribing" | "generating">("idle");

  // Settings loaded from SQLite
  let defaultLlm = $state("");
  let defaultTranscription = $state("");
  let thinking = $state(false);

  const opId = "new-session";

  function formatEta(seconds: number): string {
    if (seconds < 60) return `${Math.round(seconds)}s`;
    if (seconds < 3600) {
      const m = Math.floor(seconds / 60);
      const s = Math.round(seconds % 60);
      return s > 0 ? `${m}m ${s}s` : `${m}m`;
    }
    const h = Math.floor(seconds / 3600);
    const m = Math.round((seconds % 3600) / 60);
    return m > 0 ? `${h}h ${m}m` : `${h}h`;
  }

  // Load formats + settings
  $effect(() => {
    (async () => {
      const ok = await ensureSidecar();
      if (!ok) return;
      try {
        formats = await listFormats();
        if (formats.length > 0) selectedFormat = formats[0].name;
      } catch {}
      formatsLoaded = true;

      const s = await loadSettings();
      if (s.defaultLlm) defaultLlm = s.defaultLlm;
      if (s.defaultTranscription) defaultTranscription = s.defaultTranscription;
      if (s.defaultFormat) selectedFormat = s.defaultFormat;
      thinking = s.thinking;
    })();
  });

  // Cleanup progress on unmount
  $effect(() => {
    return () => {
      if ($currentOperation === opId) {
        progressPercent.set(0);
        progressStage.set("");
        currentOperation.set(null);
      }
    };
  });

  async function pickFile() {
    try {
      const path = await invoke<string | null>("pick_audio_file");
      if (path) audioPath = path;
    } catch (e) {
      error = String(e);
    }
  }

  async function start() {
    if (!audioPath) {
      error = "Please select an audio file.";
      return;
    }

    const ok = await ensureSidecar();
    if (!ok) {
      error = "Failed to start the processing engine.";
      return;
    }

    error = "";
    currentOperation.set(opId);
    progressPercent.set(0);
    progressStage.set("Transcribing...");
    phase = "transcribing";

    let transcript = "";
    let language = "";
    let duration: number | null = null;

    try {
      const result = await transcribe(
        audioPath,
        defaultTranscription || undefined
      );
      transcript = result.transcript;
      language = result.language;
      duration = result.duration;
    } catch (e) {
      error = `Transcription failed: ${e}`;
      phase = "idle";
      progressStage.set("");
      currentOperation.set(null);
      return;
    }

    // Create session in DB
    let session: Session;
    try {
      session = await invoke<Session>("create_session", {
        data: {
          patient_id: patientId,
          date: new Date().toISOString().slice(0, 10),
          audio_file: audioPath,
        },
      });
      await invoke("update_session", {
        data: {
          id: session.id,
          transcript,
          language: language || null,
          duration_seconds: duration,
          transcription_model: defaultTranscription || null,
        },
      });
      session = {
        ...session,
        transcript,
        language,
        duration_seconds: duration,
        transcription_model: defaultTranscription || null,
      };
    } catch (e) {
      error = `Failed to save session: ${e}`;
      phase = "idle";
      progressStage.set("");
      currentOperation.set(null);
      return;
    }

    // Generate note
    progressPercent.set(30);
    progressStage.set("Generating note...");
    phase = "generating";

    try {
      const result = await generateNote(
        transcript,
        selectedFormat,
        defaultLlm || undefined,
        thinking
      );
      await invoke("update_session", {
        data: {
          id: session.id,
          note: result.note,
          note_format: selectedFormat,
          llm_model: defaultLlm || null,
        },
      });
      session = {
        ...session,
        note: result.note,
        note_format: selectedFormat,
        llm_model: defaultLlm || null,
      };
    } catch (e) {
      error = `Note generation failed: ${e}`;
      onComplete(session);
      return;
    }

    progressPercent.set(100);
    progressStage.set("");
    currentOperation.set(null);
    phase = "idle";
    onComplete(session);
  }
</script>

<div class="new-session-panel">
  <h3>New Session</h3>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div class="new-session-row">
    <div class="form-group" style="flex: 0 0 140px;">
      <label for="format">Format</label>
      <select id="format" bind:value={selectedFormat} disabled={phase !== "idle" || !formatsLoaded}>
        {#if !formatsLoaded}
          <option value="">Loading...</option>
        {:else if formats.length === 0}
          <option value="">No formats available</option>
        {:else}
          {#each formats as f}
            <option value={f.name}>{f.name.toUpperCase()}</option>
          {/each}
        {/if}
      </select>
    </div>
    <div class="form-group" style="flex: 1;">
      <label for="audio">Audio File</label>
      <div class="file-picker-row">
        <input
          bind:value={audioPath}
          placeholder="Select an audio file..."
          readonly
          disabled={phase !== "idle"}
        />
        <button class="btn" onclick={pickFile} disabled={phase !== "idle"}>Browse</button>
      </div>
    </div>
  </div>

  {#if phase !== "idle"}
    <div class="progress-bar">
      <div class="progress-bar-fill" style="width: {$currentOperation === opId ? $progressPercent : 0}%;"></div>
    </div>
    <div class="progress-label">
      {$currentOperation === opId ? $progressStage : ""}
      {#if $currentOperation === opId && $progressEta != null && $progressEta > 0 && phase === "transcribing"}
        <span class="progress-eta">~{formatEta($progressEta)} remaining</span>
      {/if}
    </div>
  {/if}

  <div class="new-session-actions">
    <button class="btn btn-primary" onclick={start} disabled={phase !== "idle" || !audioPath || !formatsLoaded}>
      {#if phase === "transcribing"}
        Transcribing...
      {:else if phase === "generating"}
        Generating Note...
      {:else}
        Start
      {/if}
    </button>
    <button class="btn" onclick={onCancel} disabled={phase !== "idle"}>Cancel</button>
  </div>
</div>

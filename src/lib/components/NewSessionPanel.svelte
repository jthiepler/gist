<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { transcribe, generateNote, listFormats } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import { progressPercent, progressStage } from "$lib/stores";
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
  let error = $state("");
  let phase = $state<"idle" | "transcribing" | "generating">("idle");

  // Load formats
  $effect(() => {
    (async () => {
      const ok = await ensureSidecar();
      if (!ok) return;
      try {
        formats = await listFormats();
      } catch {}
    })();
  });

  // Cleanup progress on unmount
  $effect(() => {
    return () => {
      progressPercent.set(0);
      progressStage.set("");
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
    progressPercent.set(0);
    progressStage.set("Transcribing...");
    phase = "transcribing";

    let transcript = "";
    let language = "";
    let duration: number | null = null;

    try {
      const result = await transcribe(audioPath);
      transcript = result.transcript;
      language = result.language;
      duration = result.duration;
    } catch (e) {
      error = `Transcription failed: ${e}`;
      phase = "idle";
      progressStage.set("");
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
        },
      });
      // Update local session object
      session = { ...session, transcript, language, duration_seconds: duration };
    } catch (e) {
      error = `Failed to save session: ${e}`;
      phase = "idle";
      progressStage.set("");
      return;
    }

    // Generate note
    progressPercent.set(30);
    progressStage.set("Generating note...");
    phase = "generating";

    try {
      const result = await generateNote(transcript, selectedFormat);
      await invoke("update_session", {
        data: {
          id: session.id,
          note: result.note,
          note_format: selectedFormat,
        },
      });
      session = {
        ...session,
        note: result.note,
        note_format: selectedFormat,
      };
    } catch (e) {
      // Note generation failed — session still saved with transcript
      error = `Note generation failed: ${e}`;
      // Still complete with the session (transcript is there)
      onComplete(session);
      return;
    }

    progressPercent.set(100);
    progressStage.set("");
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
      <select id="format" bind:value={selectedFormat} disabled={phase !== "idle"}>
        {#if formats.length > 0}
          {#each formats as f}
            <option value={f.name}>{f.name.toUpperCase()}</option>
          {/each}
        {:else}
          <option value="soap">SOAP</option>
          <option value="cbt">CBT</option>
          <option value="intake">Intake</option>
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
      <div class="progress-bar-fill" style="width: {$progressPercent}%;"></div>
    </div>
    <div class="progress-label">{$progressStage} ({$progressPercent}%)</div>
  {/if}

  <div class="new-session-actions">
    <button class="btn btn-primary" onclick={start} disabled={phase !== "idle" || !audioPath}>
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

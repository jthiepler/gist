<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { goto } from "$app/navigation";
  import { transcribe, listModels } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import { transcribing } from "$lib/stores";
  import type { Patient, ModelsResult } from "$lib/types";
  import ProgressBar from "$lib/components/ProgressBar.svelte";
  import Card from "$lib/components/Card.svelte";

  let audioPath = $state("");
  let transcript = $state("");
  let detectedLanguage = $state("");
  let detectedDuration = $state<number | null>(null);
  let languageInput = $state("");
  let error = $state("");
  let selectedModel = $state("whisper-base");
  let models = $state<ModelsResult | null>(null);
  let patients = $state<Patient[]>([]);
  let selectedPatientId = $state("");
  let sessionDate = $state("");
  let saving = $state(false);

  onMount(async () => {
    try {
      models = await listModels();
    } catch {
      // Sidecar not running yet — will start on transcribe
    }
    try {
      patients = await invoke<Patient[]>("list_patients");
    } catch (e) {
      error = String(e);
    }
    sessionDate = new Date().toISOString().slice(0, 10);
  });

  async function pickFile() {
    try {
      const path = await invoke<string | null>("pick_audio_file");
      if (path) audioPath = path;
    } catch (e) {
      error = String(e);
    }
  }

  async function runTranscribe() {
    if (!audioPath) {
      error = "Please select an audio file first.";
      return;
    }

    const ok = await ensureSidecar();
    if (!ok) {
      error = "Failed to start sidecar.";
      return;
    }

    // Fetch model list if not yet loaded
    if (!models) {
      try {
        models = await listModels();
      } catch (e) {
        error = String(e);
        return;
      }
    }

    error = "";
    transcript = "";
    transcribing.set(true);

    try {
      const result = await transcribe(audioPath, selectedModel, languageInput || undefined);
      transcript = result.transcript;
      detectedLanguage = result.language;
      detectedDuration = result.duration;
    } catch (e) {
      error = String(e);
    } finally {
      transcribing.set(false);
    }
  }

  async function saveSession() {
    if (!selectedPatientId) {
      error = "Please select a patient.";
      return;
    }
    saving = true;
    error = "";
    try {
      const session = await invoke<Session>("create_session", {
        data: {
          patient_id: selectedPatientId,
          date: sessionDate,
          audio_file: audioPath || null,
        },
      });
      await invoke("update_session", {
        data: {
          id: session.id,
          transcript: transcript,
          language: detectedLanguage || null,
          duration_seconds: detectedDuration,
          transcription_model: selectedModel,
        },
      });
      goto(`/sessions/${session.id}`);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  // Import Session type for saveSession return
  import type { Session } from "$lib/types";
</script>

<div class="page-header">
  <h2>Transcribe</h2>
  <p>Transcribe a session audio file</p>
</div>

{#if error}
  <div class="card" style="border-color: var(--error);">
    <p style="color: var(--error); font-size: 13px;">{error}</p>
  </div>
{/if}

<Card title="Audio File">
  <div style="display: flex; gap: 8px; align-items: center;">
    <input bind:value={audioPath} placeholder="Select an audio file..." readonly style="flex: 1;" />
    <button class="btn" onclick={pickFile}>Browse</button>
  </div>
</Card>

<Card title="Options">
  <div class="form-group">
    <label for="model">Transcription Model</label>
    <select id="model" bind:value={selectedModel}>
      {#if models}
        {#each Object.entries(models.transcription) as [name, info]}
          <option value={name}>{info.display} (~{info.size_gb} GB)</option>
        {/each}
      {:else}
        <option value={selectedModel}>{selectedModel}</option>
      {/if}
    </select>
  </div>
  <div class="form-group">
    <label for="lang">Language (optional — auto-detect if empty)</label>
    <input id="lang" bind:value={languageInput} placeholder="e.g. en, es, fr" />
  </div>
</Card>

<div style="margin-bottom: 16px;">
  <button class="btn btn-primary" onclick={runTranscribe} disabled={$transcribing}>
    {$transcribing ? "Transcribing..." : "Start Transcription"}
  </button>
</div>

<ProgressBar visible={$transcribing} />

{#if transcript}
  <Card title="Transcript">
    {#if detectedLanguage || detectedDuration}
      <p style="font-size: 12px; color: var(--text-muted); margin-bottom: 12px;">
        Detected language: {detectedLanguage || 'unknown'}
        · Duration: {detectedDuration ? `${Math.round(detectedDuration)}s` : 'unknown'}
      </p>
    {/if}
    <pre style="white-space: pre-wrap; font-size: 14px; line-height: 1.6; font-family: var(--font);">{transcript}</pre>
  </Card>

  <Card title="Save to Session">
    <div class="form-group">
      <label for="patient">Patient</label>
      <select id="patient" bind:value={selectedPatientId}>
        <option value="">— Select a patient —</option>
        {#each patients as p}
          <option value={p.id}>{p.name}</option>
        {/each}
      </select>
    </div>
    <div class="form-group">
      <label for="date">Session Date</label>
      <input id="date" type="date" bind:value={sessionDate} />
    </div>
    <button class="btn btn-primary" onclick={saveSession} disabled={saving || !selectedPatientId}>
      {saving ? "Saving..." : "Save Session"}
    </button>
  </Card>
{/if}

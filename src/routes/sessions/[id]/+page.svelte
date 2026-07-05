<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import type { Patient, Session } from "$lib/types";
  import Card from "$lib/components/Card.svelte";

  let session = $state<Session | null>(null);
  let patient = $state<Patient | null>(null);
  let loading = $state(true);
  let error = $state("");

  const sessionId = $derived($page.params.id);

  $effect(() => {
    const id = sessionId;
    loading = true;
    error = "";
    session = null;
    patient = null;

    (async () => {
      try {
        const s = await invoke<Session | null>("get_session", { id });
        if (!s) {
          error = "Session not found.";
          return;
        }
        session = s;
        const patients = await invoke<Patient[]>("list_patients");
        patient = patients.find((p) => p.id === s.patient_id) ?? null;
      } catch (e) {
        error = String(e);
      } finally {
        loading = false;
      }
    })();
  });

  async function removeSession() {
    if (!session) return;
    if (!confirm("Delete this session?")) return;
    try {
      await invoke("delete_session", { id: session.id });
      goto("/sessions");
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="page-header">
  <h2>Session {session?.date ?? ""}</h2>
  <p>
    <a href="/sessions" style="color: var(--accent);">← All Sessions</a>
    {#if patient}
      · <a href="/patients/{patient.id}" style="color: var(--accent);">{patient.name}</a>
    {/if}
  </p>
</div>

{#if error}
  <div class="card" style="border-color: var(--error);">
    <p style="color: var(--error); font-size: 13px;">{error}</p>
  </div>
{:else if loading}
  <p style="color: var(--text-muted); font-size: 13px;">Loading...</p>
{:else if session}
  {#if session.audio_file}
    <Card title="Audio File">
      <p style="font-size: 13px; font-family: monospace; word-break: break-all;">{session.audio_file}</p>
    </Card>
  {/if}

  {#if session.duration_seconds || session.language || session.transcription_model}
    <Card title="Metadata">
      <div style="font-size: 13px; display: flex; gap: 24px; flex-wrap: wrap;">
        {#if session.duration_seconds}
          <span>Duration: {Math.round(session.duration_seconds / 60)} min ({Math.round(session.duration_seconds)}s)</span>
        {/if}
        {#if session.language}
          <span>Language: {session.language}</span>
        {/if}
        {#if session.transcription_model}
          <span>Transcription: {session.transcription_model}</span>
        {/if}
        {#if session.llm_model}
          <span>LLM: {session.llm_model}</span>
        {/if}
      </div>
    </Card>
  {/if}

  {#if session.transcript}
    <Card title="Transcript">
      <pre style="white-space: pre-wrap; font-size: 14px; line-height: 1.6; font-family: var(--font);">{session.transcript}</pre>
    </Card>
  {/if}

  {#if session.note}
    <Card title="Note ({(session.note_format ?? 'unknown').toUpperCase()})">
      <pre style="white-space: pre-wrap; font-size: 14px; line-height: 1.6; font-family: var(--font);">{session.note}</pre>
    </Card>
  {:else if session.transcript}
    <Card title="Note">
      <p style="font-size: 13px; color: var(--text-muted); margin-bottom: 12px;">No note generated yet.</p>
      <a href="/notes" class="btn btn-primary" style="text-decoration: none;">Generate Note</a>
    </Card>
  {/if}

  <Card title="Actions">
    <div style="display: flex; gap: 8px; flex-wrap: wrap;">
      {#if session.transcript}
        <a href="/notes" class="btn" style="text-decoration: none;">Go to Notes</a>
      {/if}
      <button class="btn btn-danger" onclick={removeSession}>Delete Session</button>
    </div>
  </Card>
{/if}

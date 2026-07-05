<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { goto } from "$app/navigation";
  import { generateNote, listFormats, listModels } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import { generating } from "$lib/stores";
  import type { NoteFormat, Session, ModelsResult } from "$lib/types";
  import ProgressBar from "$lib/components/ProgressBar.svelte";
  import Card from "$lib/components/Card.svelte";

  let transcript = $state("");
  let selectedFormat = $state("soap");
  let formats = $state<NoteFormat[]>([]);
  let thinking = $state(true);
  let note = $state("");
  let error = $state("");
  let selectedSessionId = $state("");
  let sessions = $state<Session[]>([]);
  let saving = $state(false);
  let models = $state<ModelsResult | null>(null);
  let selectedLlm = $state("");

  onMount(async () => {
    const ok = await ensureSidecar();
    if (!ok) {
      error = "Failed to start sidecar.";
      return;
    }
    try {
      formats = await listFormats();
      models = await listModels();
      if (models && Object.keys(models.llm).length > 0) {
        selectedLlm = Object.keys(models.llm)[0];
      }
    } catch (e) {
      // Non-fatal — user can still paste transcript manually
    }
    try {
      const all = await invoke<Session[]>("list_sessions");
      sessions = all.filter((s) => s.transcript);
    } catch {}
  });

  function loadSessionTranscript() {
    const session = sessions.find((s) => s.id === selectedSessionId);
    if (session?.transcript) {
      transcript = session.transcript;
    }
  }

  async function runGenerate() {
    if (!transcript.trim()) {
      error = "Please enter or load a transcript.";
      return;
    }

    const ok = await ensureSidecar();
    if (!ok) {
      error = "Failed to start sidecar.";
      return;
    }

    error = "";
    note = "";
    generating.set(true);

    try {
      const result = await generateNote(transcript, selectedFormat, selectedLlm || undefined, thinking);
      note = result.note;
    } catch (e) {
      error = String(e);
    } finally {
      generating.set(false);
    }
  }

  async function saveNote() {
    if (!selectedSessionId) {
      error = "Please select a session to save to.";
      return;
    }
    saving = true;
    error = "";
    try {
      await invoke("update_session", {
        data: {
          id: selectedSessionId,
          note: note,
          note_format: selectedFormat,
          llm_model: selectedLlm || null,
        },
      });
      goto(`/sessions/${selectedSessionId}`);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="page-header">
  <h2>Notes</h2>
  <p>Generate clinical notes from session transcripts</p>
</div>

{#if error}
  <div class="card" style="border-color: var(--error);">
    <p style="color: var(--error); font-size: 13px;">{error}</p>
  </div>
{/if}

<Card title="Load Transcript">
  <div class="form-group">
    <label for="session">From existing session</label>
    <select id="session" bind:value={selectedSessionId} onchange={loadSessionTranscript}>
      <option value="">— Type or paste a transcript below —</option>
      {#each sessions as s}
        <option value={s.id}>{s.date} — {s.transcript?.slice(0, 60)}...</option>
      {/each}
    </select>
  </div>
</Card>

<Card title="Transcript">
  <textarea
    bind:value={transcript}
    placeholder="Paste session transcript here..."
    style="min-height: 200px;"
  ></textarea>
</Card>

<Card title="Options">
  <div class="form-group">
    <label for="fmt">Note Format</label>
    <select id="fmt" bind:value={selectedFormat}>
      {#each formats as f}
        <option value={f.name}>{f.description || f.name}</option>
      {/each}
    </select>
  </div>
  <div class="form-group">
    <label for="llm">LLM Model</label>
    <select id="llm" bind:value={selectedLlm}>
      {#if models}
        {#each Object.entries(models.llm) as [name, info]}
          <option value={name}>{info.display} (~{info.size_gb} GB)</option>
        {/each}
      {/if}
    </select>
  </div>
  <div class="toggle-row">
    <span style="font-size: 13px; font-weight: 600; color: var(--text-muted);">Reasoning (thinking)</span>
    <label class="toggle" class:active={thinking}>
      <input type="checkbox" bind:checked={thinking} style="display: none;" />
      <div class="toggle-knob"></div>
    </label>
  </div>
</Card>

<div style="margin-bottom: 16px;">
  <button class="btn btn-primary" onclick={runGenerate} disabled={$generating}>
    {$generating ? "Generating..." : "Generate Note"}
  </button>
</div>

<ProgressBar visible={$generating} />

{#if note}
  <Card title="Generated Note ({selectedFormat.toUpperCase()})">
    <pre style="white-space: pre-wrap; font-size: 14px; line-height: 1.6; font-family: var(--font);">{note}</pre>
  </Card>

  <Card title="Save">
    <button class="btn btn-primary" onclick={saveNote} disabled={saving || !selectedSessionId}>
      {saving ? "Saving..." : "Save Note to Session"}
    </button>
    {#if !selectedSessionId}
      <p style="font-size: 12px; color: var(--text-muted); margin-top: 8px;">Select a session above to save this note.</p>
    {/if}
  </Card>
{/if}

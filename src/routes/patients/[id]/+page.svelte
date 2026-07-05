<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/stores";
  import { patients } from "$lib/stores";
  import { generateNote } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import { progressPercent, progressStage } from "$lib/stores";
  import type { Patient, Session } from "$lib/types";
  import SessionCard from "$lib/components/SessionCard.svelte";
  import NewSessionPanel from "$lib/components/NewSessionPanel.svelte";

  let sessions = $state<Session[]>([]);
  let loading = $state(true);
  let error = $state("");
  let showNewSession = $state(false);
  let generatingNoteFor = $state<string | null>(null);
  let editingName = $state(false);
  let editName = $state("");
  let savingName = $state(false);

  const patientId = $derived($page.params.id);

  const patient = $derived($patients.find((p) => p.id === patientId) ?? null);

  // Re-fetch sessions when patient changes
  $effect(() => {
    const id = patientId;
    loading = true;
    error = "";
    sessions = [];
    showNewSession = false;

    (async () => {
      try {
        sessions = await invoke<Session[]>("list_sessions", { patientId: id });
      } catch (e) {
        error = String(e);
      } finally {
        loading = false;
      }
    })();
  });

  let lastSessionDate = $derived(
    sessions.length > 0
      ? new Date(sessions[0].date).toLocaleDateString("en-US", {
          year: "numeric",
          month: "short",
          day: "numeric",
        })
      : null
  );

  async function deleteSession(session: Session) {
    if (!confirm(`Delete session from ${session.date}?`)) return;
    try {
      await invoke("delete_session", { id: session.id });
      sessions = sessions.filter((s) => s.id !== session.id);
    } catch (e) {
      error = String(e);
    }
  }

  async function generateNoteForSession(session: Session) {
    if (!session.transcript) return;
    generatingNoteFor = session.id;
    error = "";
    progressPercent.set(30);
    progressStage.set("Generating note...");

    try {
      const format = session.note_format || "soap";
      const result = await generateNote(session.transcript, format);
      await invoke("update_session", {
        data: {
          id: session.id,
          note: result.note,
          note_format: format,
        },
      });
      sessions = sessions.map((s) =>
        s.id === session.id
          ? { ...s, note: result.note, note_format: format }
          : s
      );
    } catch (e) {
      error = `Note generation failed: ${e}`;
    } finally {
      generatingNoteFor = null;
      progressStage.set("");
      progressPercent.set(0);
    }
  }

  function onNewSessionComplete(session: Session) {
    sessions = [session, ...sessions];
    showNewSession = false;
  }

  function startEditName() {
    if (!patient) return;
    editName = patient.name;
    editingName = true;
  }

  function cancelEditName() {
    editingName = false;
    editName = "";
  }

  async function saveEditName() {
    if (!patient || !editName.trim()) return;
    savingName = true;
    try {
      await invoke("update_patient", { data: { id: patient.id, name: editName.trim() } });
      patients.update((list) =>
        list.map((p) => (p.id === patient.id ? { ...p, name: editName.trim() } : p))
      );
      editingName = false;
    } catch (e) {
      error = String(e);
    } finally {
      savingName = false;
    }
  }
</script>

{#if loading}
  <p class="text-muted">Loading...</p>
{:else if !patient}
  <div class="error-banner">Patient not found.</div>
{:else}
  <div class="workspace-header">
    {#if editingName}
      <div class="name-edit-row">
        <input
          bind:value={editName}
          class="name-edit-input"
          onkeydown={(e) => {
            if (e.key === 'Enter') saveEditName();
            if (e.key === 'Escape') cancelEditName();
          }}
          disabled={savingName}
          autofocus
        />
        <button class="btn btn-sm btn-primary" onclick={saveEditName} disabled={savingName || !editName.trim()}>
          Save
        </button>
        <button class="btn btn-sm" onclick={cancelEditName} disabled={savingName}>Cancel</button>
      </div>
    {:else}
      <div class="name-display-row">
        <h2>{patient.name}</h2>
        <button class="btn-ghost btn-sm edit-name-btn" onclick={startEditName} title="Edit name" aria-label="Edit name">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>
          </svg>
        </button>
      </div>
    {/if}
    <div class="header-meta">
      {#if lastSessionDate}
        Last session: {lastSessionDate} · {sessions.length} {sessions.length === 1 ? 'session' : 'sessions'}
      {:else}
        No sessions yet
      {/if}
    </div>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div class="workspace-toolbar">
    <div></div>
    {#if !showNewSession}
      <button class="btn btn-primary" onclick={() => showNewSession = true}>
        + New Session
      </button>
    {/if}
  </div>

  {#if showNewSession}
    <NewSessionPanel
      {patientId}
      onComplete={onNewSessionComplete}
      onCancel={() => showNewSession = false}
    />
  {/if}

  {#if sessions.length === 0 && !showNewSession}
    <div class="empty-state">
      <div class="empty-title">No Sessions</div>
      <div class="empty-desc">Click "+ New Session" to transcribe an audio recording and generate a note.</div>
    </div>
  {:else}
    <div class="session-list">
      {#each sessions as session (session.id)}
        <SessionCard
          {session}
          onGenerateNote={generateNoteForSession}
          onDelete={deleteSession}
        />
      {/each}
    </div>
  {/if}
{/if}

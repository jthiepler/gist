<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { patients, sidecarBusy, activeOperation } from "$lib/stores";
  import { generateNote, listNoteFormats, getPatientFormats, createSessionNote } from "$lib/rpc";
  import { progressPercent, progressStage, progressBase, progressScale, currentOperation } from "$lib/stores";
  import { loadSettings } from "$lib/settings";
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

  const patientId = $derived($page.params.id ?? "");

  const patient = $derived($patients.find((p) => p.id === patientId) ?? null);

  // Re-fetch sessions when patient changes
  $effect(() => {
    const id = patientId;
    let stale = false;
    loading = true;
    error = "";
    sessions = [];
    showNewSession = false;

    (async () => {
      try {
        const result = await invoke<Session[]>("list_sessions", { patientId: id });
        if (stale) return;
        sessions = result;
      } catch (e) {
        if (stale) return;
        error = String(e);
      } finally {
        if (!stale) loading = false;
      }
    })();

    return () => { stale = true; };
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
    if (!(await confirm(`Delete session from ${session.date}?`))) return;
    try {
      await invoke("delete_session", { id: session.id });
      sessions = sessions.filter((s) => s.id !== session.id);
    } catch (e) {
      error = String(e);
    }
  }

  async function generateNoteForSession(session: Session) {
    if (!session.transcript) return;
    if ($sidecarBusy) {
      error = "Another operation is in progress. Please wait or cancel it first.";
      return;
    }
    generatingNoteFor = session.id;
    error = "";

    // Load settings
    const settings = await loadSettings();
    const llm = settings.defaultLlm;
    const thinking = settings.thinking;

    // Determine formats: patient's saved selection, or default to "soap"
    let formatsToGenerate = await getPatientFormats(patientId);
    if (formatsToGenerate.length === 0) {
      formatsToGenerate = ["soap"];
    }

    // Filter out formats that already have notes for this session
    const existingFormats = new Set(session.notes.map((n) => n.format));
    const missing = formatsToGenerate.filter((f) => !existingFormats.has(f)).sort((a, b) => a.localeCompare(b));

    if (missing.length === 0) {
      error = "All selected formats already have notes for this session.";
      generatingNoteFor = null;
      return;
    }

    const opId = `gen-note-${session.id}`;
    currentOperation.set(opId);
    progressBase.set(0);
    progressScale.set(100);
    progressPercent.set(30);
    progressStage.set("Generating notes...");
    activeOperation.set({ type: "generate_note", label: "Generating notes..." });

    // Load templates for prompts
    let templates = await listNoteFormats();

    const totalNotes = missing.length;
    const basePct = 30;
    const noteRange = 70;
    let updatedNotes = [...session.notes];

    try {
      for (let i = 0; i < missing.length; i++) {
        const fmtName = missing[i];
        const label = `Generating ${fmtName.toUpperCase()} note (${i + 1}/${totalNotes})...`;
        progressStage.set(label);
        activeOperation.set({ type: "generate_note", label });
        const fmtBase = basePct + Math.round((i / totalNotes) * noteRange);
        const fmtScale = Math.round(noteRange / totalNotes);
        progressBase.set(fmtBase);
        progressScale.set(fmtScale);

        const tmpl = templates.find((t) => t.name === fmtName);
        const result = await generateNote(
          session.transcript!,
          fmtName,
          llm || undefined,
          thinking,
          tmpl?.prompt
        );
        const sn = await createSessionNote(session.id, fmtName, result.note, llm || null);
        updatedNotes = [...updatedNotes, sn].sort((a, b) => a.format.localeCompare(b.format));
        // Live-update sessions list so user sees progress
        sessions = sessions.map((s) =>
          s.id === session.id ? { ...s, notes: updatedNotes } : s
        );
      }
    } catch (e) {
      const msg = String(e);
      if (msg === "sidecar_busy") {
        error = "Another operation is in progress. Please wait or cancel it first.";
      } else {
        error = `Note generation failed: ${msg}`;
      }
    } finally {
      generatingNoteFor = null;
      currentOperation.set(null);
      activeOperation.set({ type: null, label: "" });
      progressStage.set("");
      progressPercent.set(0);
      progressBase.set(0);
      progressScale.set(100);
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

  async function deletePatient() {
    if (!patient) return;
    if (!(await confirm(`Delete "${patient.name}"? All related sessions and notes will be permanently deleted. This cannot be undone.`, { title: "Delete patient", kind: "warning" }))) return;
    try {
      await invoke("delete_patient", { id: patient.id });
      patients.update((list) => list.filter((p) => p.id !== patient.id));
      goto("/");
    } catch (e) {
      error = String(e);
    }
  }
</script>

{#if loading}
  <p class="text-muted">Loading...</p>
{:else if !patient}
  <div class="error-banner">Patient not found.</div>
{:else}
  <div class="workspace-header">
    <div class="header-top-row">
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
      <button class="btn btn-sm btn-danger delete-patient-btn" onclick={deletePatient} title="Delete patient" aria-label="Delete patient">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"/>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
        </svg>
      </button>
    </div>
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
          isGenerating={generatingNoteFor === session.id}
          onGenerateNote={generateNoteForSession}
          onDelete={deleteSession}
        />
      {/each}
    </div>
  {/if}
{/if}

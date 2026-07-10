<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { tick, untrack } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import {
    activeOperation,
    currentOperation,
    isRecording,
    patients,
    pendingSession,
    recordingContext,
    sessionUpdate,
    sidecarBusy,
  } from "$lib/stores";
  import { generateNote, listNoteFormats, getPatientFormats, createSessionNote } from "$lib/rpc";
  import { formatInputsForNoteGeneration, hasNoteSourceMaterial } from "$lib/sessionInputs";
  import { progressBase, progressPercent, progressScale, progressStage } from "$lib/stores";
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
  let editNameInput = $state<HTMLInputElement | null>(null);
  let savingName = $state(false);
  let showPatientMenu = $state(false);
  let recordingNewSession = $state(false);

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

  // Keep the New Session panel open only for recordings that create a new session.
  // Existing-session recordings are controlled from their source-material pane.
  $effect(() => {
    const ctx = $recordingContext;
    if ($isRecording && ctx?.patientId === patientId) {
      recordingNewSession = !ctx.session;
      showNewSession = recordingNewSession;
      return;
    }

    if (!$isRecording && recordingNewSession) {
      recordingNewSession = false;
      showNewSession = false;
    }
  });

  // Upsert a session into the list without triggering reactive loops
  function upsertSession(s: Session) {
    const exists = untrack(() => sessions.some((x) => x.id === s.id));
    if (exists) {
      sessions = untrack(() =>
        sessions.map((x) => (x.id === s.id ? s : x)),
      );
    } else {
      sessions = untrack(() => [s, ...sessions]);
    }
  }

  // Live-update: fires at each step of processing (source material saved, each note)
  $effect(() => {
    const s = $sessionUpdate;
    if (s && s.patient_id === patientId) {
      upsertSession(s);
    }
  });

  // Watch for sessions processed by the layout (from recording-stopped)
  $effect(() => {
    const s = $pendingSession;
    if (s && s.patient_id === patientId) {
      upsertSession(s);
      pendingSession.set(null);
      showNewSession = false;
    }
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

  async function generateNoteForSession(
    session: Session,
    options: { regenerateExisting?: boolean } = {}
  ) {
    if (!hasNoteSourceMaterial(session)) return;
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

    const existingFormats = new Set(session.notes.map((n) => n.format));
    const formatsToRefresh = formatsToGenerate
      .filter((f) => options.regenerateExisting || !existingFormats.has(f))
      .sort((a, b) => a.localeCompare(b));

    if (formatsToRefresh.length === 0) {
      error = "All selected note formats already exist for this session.";
      generatingNoteFor = null;
      return;
    }

    const opId = `gen-note-${session.id}`;
    currentOperation.set(opId);
    progressBase.set(0);
    progressScale.set(100);
    progressPercent.set(30);
    progressStage.set(options.regenerateExisting ? "Updating documentation..." : "Creating documentation...");
    activeOperation.set({
      type: "generate_note",
      label: options.regenerateExisting ? "Updating documentation..." : "Creating documentation...",
    });
    const noteSourceMaterial = formatInputsForNoteGeneration(session);

    // Load templates for prompts
    let templates = await listNoteFormats();

    const totalNotes = formatsToRefresh.length;
    const basePct = 30;
    const noteRange = 70;
    let updatedNotes = [...session.notes];

    try {
      for (let i = 0; i < formatsToRefresh.length; i++) {
        const fmtName = formatsToRefresh[i];
        const label = `Generating ${fmtName.toUpperCase()} note (${i + 1}/${totalNotes})...`;
        progressStage.set(label);
        activeOperation.set({ type: "generate_note", label });
        const fmtBase = basePct + Math.round((i / totalNotes) * noteRange);
        const fmtScale = Math.round(noteRange / totalNotes);
        progressBase.set(fmtBase);
        progressScale.set(fmtScale);

        const tmpl = templates.find((t) => t.name === fmtName);
        const result = await generateNote(
          noteSourceMaterial,
          fmtName,
          llm || undefined,
          thinking,
          tmpl?.prompt
        );
        const sn = await createSessionNote(session.id, fmtName, result.note, llm || null);
        updatedNotes = [
          ...updatedNotes.filter((note) => note.format !== sn.format),
          sn,
        ].sort((a, b) => a.format.localeCompare(b.format));
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

  function updateSessionInList(session: Session) {
    sessions = sessions.map((s) => (s.id === session.id ? session : s));
  }

  async function startEditName() {
    if (!patient) return;
    showPatientMenu = false;
    editName = patient.name;
    editingName = true;
    await tick();
    editNameInput?.focus();
    editNameInput?.select();
  }

  function cancelEditName() {
    editingName = false;
    editName = "";
    showPatientMenu = false;
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
    showPatientMenu = false;
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
            bind:this={editNameInput}
            bind:value={editName}
            class="name-edit-input"
            onkeydown={(e) => {
              if (e.key === 'Enter') saveEditName();
              if (e.key === 'Escape') cancelEditName();
            }}
            disabled={savingName}
          />
          <button class="btn btn-sm btn-primary" onclick={saveEditName} disabled={savingName || !editName.trim()}>
            Save
          </button>
          <button class="btn btn-sm" onclick={cancelEditName} disabled={savingName}>Cancel</button>
        </div>
      {:else}
        <div class="name-display-row">
          <h2>{patient.name}</h2>
        </div>
      {/if}
      {#if !editingName}
        <div class="patient-menu">
          <button
            class="icon-btn patient-menu-trigger"
            onclick={() => showPatientMenu = !showPatientMenu}
            title="Patient actions"
            aria-label="Patient actions"
            aria-expanded={showPatientMenu}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="1"/>
              <circle cx="19" cy="12" r="1"/>
              <circle cx="5" cy="12" r="1"/>
            </svg>
          </button>
          {#if showPatientMenu}
            <div class="patient-menu-popover">
              <button class="patient-menu-item" onclick={startEditName}>Edit name</button>
              <button class="patient-menu-item danger" onclick={deletePatient}>Delete patient</button>
            </div>
          {/if}
        </div>
      {/if}
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
        <div class="empty-title">No sessions yet</div>
        <div class="empty-desc">Start by recording a session, uploading audio, pasting a transcript, or adding a clinician note.</div>
    </div>
  {:else}
    <div class="session-list">
      {#each sessions as session (session.id)}
        <SessionCard
          {session}
          isGenerating={generatingNoteFor === session.id}
          onGenerateNote={generateNoteForSession}
          onDelete={deleteSession}
          onChange={updateSessionInList}
        />
      {/each}
    </div>
  {/if}
{/if}

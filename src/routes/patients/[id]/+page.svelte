<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { onMount, tick, untrack } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import {
    activeOperation,
    currentOperation,
    isRecording,
    patients,
    recordingContext,
    recordingJobsProcessing,
    sessionUpdate,
    sidecarBusy,
    progressBase,
    progressPercent,
    progressScale,
    progressStage,
  } from "$lib/stores";
  import { getPatientFormats } from "$lib/rpc";
  import { hasNoteSourceMaterial } from "$lib/sessionInputs";
  import { selectNoteFormats, type NoteGenerationSelection } from "$lib/noteGeneration";
  import { generateSessionDocumentation } from "$lib/documentation";
  import { loadSettings } from "$lib/settings";
  import { formatLocalDate } from "$lib/date";
  import { isNewSessionRecording } from "$lib/releaseGuards";
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
  let destructiveActionsBlocked = $derived(
    $isRecording ||
      $sidecarBusy ||
      $recordingJobsProcessing ||
      $currentOperation !== null ||
      generatingNoteFor !== null,
  );

  onMount(() => {
    const dismissMenu = (event: PointerEvent) => {
      if (!(event.target as Element).closest(".patient-menu")) showPatientMenu = false;
    };
    document.addEventListener("pointerdown", dismissMenu);
    return () => document.removeEventListener("pointerdown", dismissMenu);
  });

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
        const liveSessions = untrack(() => sessions);
        const fetchedIds = new Set(result.map((session) => session.id));
        const liveById = new Map(liveSessions.map((session) => [session.id, session]));
        sessions = sortSessions([
          ...result.map((session) => liveById.get(session.id) ?? session),
          ...liveSessions.filter((session) => !fetchedIds.has(session.id)),
        ]);
      } catch (e) {
        if (stale) return;
        error = "Gist could not safely load this patient's sessions. Nothing has been changed. Try again after reopening Gist; if this continues, restore the Gist data folder from a backup before editing or deleting anything.";
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
      recordingNewSession = isNewSessionRecording(ctx);
      showNewSession = recordingNewSession;
      return;
    }

    if (!$isRecording && recordingNewSession) {
      recordingNewSession = false;
      showNewSession = false;
    }
  });

  function sortSessions(items: Session[]) {
    return [...items].sort((a, b) => {
      const aKey = `${a.date.slice(0, 10)}T${a.start_time ?? ""}`;
      const bKey = `${b.date.slice(0, 10)}T${b.start_time ?? ""}`;
      return bKey.localeCompare(aKey) || b.created_at.localeCompare(a.created_at);
    });
  }

  // Upsert without subscribing the surrounding effect to the session array.
  function upsertSession(nextSession: Session) {
    sessions = untrack(() => sortSessions([
      nextSession,
      ...sessions.filter((session) => session.id !== nextSession.id),
    ]));
  }

  // Live-update: fires at each step of processing (source material saved, each note)
  $effect(() => {
    const s = $sessionUpdate;
    if (s && s.patient_id === patientId) {
      upsertSession(s);
    }
  });

  let lastSessionDate = $derived(
    sessions.length > 0
      ? formatLocalDate(sessions[0].date, {
          year: "numeric",
          month: "short",
          day: "numeric",
        }, "en-US")
      : null
  );

  let sessionGroups = $derived.by(() => {
    const groups = new Map<string, Session[]>();
    for (const session of sessions) {
      const key = session.date.slice(0, 10);
      groups.set(key, [...(groups.get(key) ?? []), session]);
    }
    return [...groups.entries()].map(([date, items]) => ({
      date,
      label: new Date(`${date}T12:00:00`).toLocaleDateString([], {
        weekday: "short",
        year: "numeric",
        month: "long",
        day: "numeric",
      }),
      sessions: items,
    }));
  });

  async function deleteSession(session: Session) {
    if (destructiveActionsBlocked) {
      error = "Finish the active recording or processing task before deleting a session.";
      return;
    }
    const formattedDate = formatLocalDate(session.date, {
      year: "numeric",
      month: "long",
      day: "numeric",
    }, "en-US");
    if (!(await confirm(`Delete the session from ${formattedDate}?`, {
      title: "Delete session",
      kind: "warning",
    }))) return;
    try {
      await invoke("delete_session", { id: session.id });
      sessions = sessions.filter((s) => s.id !== session.id);
    } catch (e) {
      error = String(e);
    }
  }

  async function generateNoteForSession(
    session: Session,
    options: NoteGenerationSelection = {}
  ): Promise<boolean> {
    if (!hasNoteSourceMaterial(session)) return false;
    if ($sidecarBusy) {
      error = "Another operation is in progress. Please wait or cancel it first.";
      return false;
    }
    generatingNoteFor = session.id;
    error = "";
    try {
      const settings = await loadSettings();
      const preferredFormats = options.formats || options.regenerateExisting
        ? []
        : await getPatientFormats(patientId);
      const formatsToRefresh = selectNoteFormats(
        session.notes.map((note) => note.format),
        preferredFormats,
        options,
      );
      if (formatsToRefresh.length === 0) {
        error = "All selected note formats already exist for this session.";
        return false;
      }

      currentOperation.set(`gen-note-${session.id}`);
      progressBase.set(0);
      progressScale.set(100);
      progressPercent.set(30);
      progressStage.set(options.regenerateExisting ? "Updating notes..." : "Creating notes...");
      activeOperation.set({
        type: "generate_note",
        label: options.regenerateExisting ? "Updating notes..." : "Creating notes...",
      });
      await generateSessionDocumentation(session, formatsToRefresh, {
        defaultLlm: settings.defaultLlm,
        thinking: false,
        verb: "Generating",
        onSessionUpdate: upsertSession,
      });
      return true;
    } catch (e) {
      const msg = String(e);
      if (msg === "sidecar_busy") {
        error = "Another operation is in progress. Please wait or cancel it first.";
      } else {
        error = `Note generation failed: ${msg}`;
      }
      return false;
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

  function onNewSessionProcessingStart(session: Session) {
    upsertSession(session);
  }

  function onNewSessionComplete(session: Session) {
    upsertSession(session);
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
    if (destructiveActionsBlocked) {
      error = "Finish the active recording or processing task before deleting this patient.";
      return;
    }
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
  <p class="text-muted">Loading sessions...</p>
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
        <div class="patient-header-actions">
          {#if !showNewSession}
            <button class="btn btn-primary" onclick={() => showNewSession = true}>New session</button>
          {/if}
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
              <div class="patient-menu-popover" role="menu" aria-label="Patient actions">
                <button class="patient-menu-item" role="menuitem" onclick={startEditName}>Edit patient name</button>
                <button
                  class="patient-menu-item danger"
                  role="menuitem"
                  onclick={deletePatient}
                  disabled={destructiveActionsBlocked}
                >Delete patient</button>
              </div>
            {/if}
          </div>
        </div>
      {/if}
    </div>
    <div class="header-meta">
      {#if lastSessionDate}
        {sessions.length} {sessions.length === 1 ? 'session' : 'sessions'} · Last seen: {lastSessionDate}
      {:else}
        No sessions yet
      {/if}
    </div>
  </div>

  {#if error}
    <div class="error-banner" role="alert">
      <span>{error}</span>
      <button class="btn btn-sm" onclick={() => window.location.reload()}>Reload</button>
    </div>
  {/if}

  {#if showNewSession}
    <NewSessionPanel
      {patientId}
      onComplete={onNewSessionComplete}
      onProcessingStart={onNewSessionProcessingStart}
      onSessionUpdate={upsertSession}
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
      {#each sessionGroups as group, groupIndex (group.date)}
        <section class="session-group" aria-labelledby={`session-group-${group.date}`}>
          <h3 id={`session-group-${group.date}`} class="session-group-heading">{group.label}</h3>
          <div class="session-group-items">
            {#each group.sessions as session, sessionIndex (session.id)}
              <SessionCard
                {session}
                initiallyExpanded={groupIndex === 0 && sessionIndex === 0}
                isGenerating={generatingNoteFor === session.id}
                deletionDisabled={destructiveActionsBlocked}
                onGenerateNote={generateNoteForSession}
                onDelete={deleteSession}
                onChange={updateSessionInList}
              />
            {/each}
          </div>
        </section>
      {/each}
    </div>
  {/if}
{/if}

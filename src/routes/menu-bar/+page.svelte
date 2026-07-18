<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    getPatientFormats,
    getRecordingJob,
    getRecordingState,
    getSession,
    listAudioDevices,
    listNoteFormats,
    onRecordingError,
    onRecordingStopped,
    onRecordingTick,
    pauseRecording,
    resumeRecording,
    startRecording,
    stopRecording,
    type AudioDevice,
  } from "$lib/rpc";
  import { loadSettings } from "$lib/settings";
  import {
    DEFAULT_DIARIZATION_SPEAKERS,
    DIARIZATION_SPEAKER_COUNTS,
  } from "$lib/diarization";
  import { DEFAULT_LLM } from "$lib/models";
  import type { Patient, Session, SessionInputKind } from "$lib/types";

  type View = "patients" | "target" | "recording" | "saved" | "failed";
  type TargetMode = "new" | "existing";

  const now = new Date();
  let view = $state<View>("patients");
  let patients = $state<Patient[]>([]);
  let patientSearch = $state("");
  let selectedPatient = $state<Patient | null>(null);
  let sessions = $state<Session[]>([]);
  let selectedSessionId = $state("");
  let recordingType = $state<SessionInputKind>("session_transcript");
  let targetMode = $state<TargetMode>("new");
  let sessionDate = $state(now.toLocaleDateString("en-CA"));
  let sessionTime = $state(
    `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`,
  );
  let sessionTitle = $state("");
  let numSpeakers = $state<number>(DEFAULT_DIARIZATION_SPEAKERS);
  let inputDevices = $state<AudioDevice[]>([]);
  let outputDevices = $state<AudioDevice[]>([]);
  let selectedInputDevice = $state("");
  let selectedOutputDevice = $state("");
  let selectedFormats = $state<string[]>([]);
  let defaultLlm = $state(DEFAULT_LLM);
  let loading = $state(true);
  let loadingTarget = $state(false);
  let starting = $state(false);
  let recordingAction = $state<"pause" | "stop" | null>(null);
  let error = $state("");
  let isRecording = $state(false);
  let isPaused = $state(false);
  let elapsed = $state(0);
  let level = $state(0);
  let activeSession = $state<Session | null>(null);
  let activePatientName = $state("");
  let savedNeedsReview = $state(false);
  let unlisteners: UnlistenFn[] = [];
  let destroyed = false;
  let targetLoadVersion = 0;

  let filteredPatients = $derived(
    patients.filter((patient) =>
      patient.name.toLocaleLowerCase().includes(patientSearch.trim().toLocaleLowerCase()),
    ),
  );
  let selectedExistingSession = $derived(
    sessions.find((session) => session.id === selectedSessionId) ?? null,
  );
  let isSessionRecording = $derived(recordingType === "session_transcript");
  let canStart = $derived(
    !starting &&
      selectedInputDevice.length > 0 &&
      (!isSessionRecording || selectedOutputDevice.length > 0) &&
      (targetMode === "new" || selectedExistingSession !== null),
  );

  function formatElapsed(seconds: number): string {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const remaining = Math.floor(seconds % 60);
    if (hours > 0) {
      return `${hours}:${String(minutes).padStart(2, "0")}:${String(remaining).padStart(2, "0")}`;
    }
    return `${minutes}:${String(remaining).padStart(2, "0")}`;
  }

  function formatSessionDate(session: Session): string {
    return new Date(`${session.date.slice(0, 10)}T12:00:00`).toLocaleDateString([], {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  }

  function formatSessionTitle(session: Session): string {
    return session.title?.trim() || "Untitled session";
  }

  function formatSessionMeta(session: Session): string {
    return [formatSessionDate(session), session.start_time].filter(Boolean).join(" · ");
  }

  function formatSession(session: Session): string {
    return [formatSessionTitle(session), formatSessionMeta(session)].join(" · ");
  }

  async function closePopover() {
    try {
      await invoke("hide_menu_bar_window");
    } catch (reason) {
      error = `Could not close the menu: ${String(reason)}`;
    }
  }

  async function openGist() {
    try {
      await invoke("show_main_app");
    } catch (reason) {
      error = `Could not open Gist: ${String(reason)}`;
    }
  }

  async function refreshRecordingState() {
    try {
      const state = await getRecordingState();
      isRecording = state.is_recording;
      isPaused = state.is_paused;
      elapsed = state.elapsed_seconds;
      if (!state.is_recording) {
        if (view === "recording") view = "saved";
        return;
      }

      view = "recording";
      if (state.job_id) {
        const job = await getRecordingJob(state.job_id);
        activeSession = await getSession(job.session_id);
        activePatientName =
          patients.find((patient) => patient.id === activeSession?.patient_id)?.name ?? "Session";
      }
    } catch (reason) {
      error = `Could not read recording state: ${String(reason)}`;
    }
  }

  async function refreshPatients() {
    patients = await invoke<Patient[]>("list_patients");
  }

  async function refreshPopoverData() {
    try {
      await refreshPatients();
      await refreshRecordingState();
    } catch (reason) {
      error = `Could not refresh Gist: ${String(reason)}`;
    }
  }

  async function initialize() {
    try {
      await refreshPatients();
      await refreshRecordingState();
    } catch (reason) {
      error = `Could not load Gist: ${String(reason)}`;
    } finally {
      loading = false;
    }

    const listeners: UnlistenFn[] = [];
    try {
      listeners.push(await listen("menu-bar-opened", () => {
        void refreshPopoverData();
      }));
      listeners.push(await onRecordingTick((data) => {
        isRecording = true;
        isPaused = data.is_paused;
        elapsed = data.elapsed_seconds;
        level = data.level;
        view = "recording";
      }));
      listeners.push(await onRecordingStopped((data) => {
        recordingAction = null;
        isRecording = false;
        isPaused = false;
        elapsed = 0;
        level = 0;
        error = "";
        savedNeedsReview = data.is_short_recording;
        view = "saved";
      }));
      listeners.push(await onRecordingError((data) => {
        recordingAction = null;
        isRecording = false;
        isPaused = false;
        level = 0;
        savedNeedsReview = false;
        error = data.message;
        view = "failed";
      }));
    } catch (reason) {
      listeners.forEach((unlisten) => unlisten());
      if (!destroyed) error = `Could not monitor recording events: ${String(reason)}`;
      return;
    }
    if (destroyed) {
      listeners.forEach((unlisten) => unlisten());
    } else {
      unlisteners = listeners;
    }
  }

  onMount(() => {
    void initialize();
  });

  onDestroy(() => {
    destroyed = true;
    unlisteners.forEach((unlisten) => unlisten());
  });

  async function selectPatient(patient: Patient) {
    const loadVersion = ++targetLoadVersion;
    selectedPatient = patient;
    selectedSessionId = "";
    targetMode = "new";
    error = "";
    loadingTarget = true;
    view = "target";
    try {
      const [patientSessions, devices, formats, savedFormats, settings] = await Promise.all([
        invoke<Session[]>("list_sessions", { patientId: patient.id }),
        listAudioDevices(),
        listNoteFormats(),
        getPatientFormats(patient.id),
        loadSettings(),
      ]);
      if (loadVersion !== targetLoadVersion || destroyed) return;
      sessions = patientSessions;
      inputDevices = devices.filter((device) => device.device_type === "input");
      outputDevices = devices.filter((device) => device.device_type === "output");
      selectedInputDevice = inputDevices[0]?.id ?? "";
      selectedOutputDevice = outputDevices[0]?.id ?? "";
      const visibleFormats = formats.filter((format) => !format.hidden);
      const visibleNames = new Set(visibleFormats.map((format) => format.name));
      selectedFormats = savedFormats
        ? savedFormats.filter((format) => visibleNames.has(format))
        : visibleFormats.slice(0, 1).map((format) => format.name);
      defaultLlm = settings.defaultLlm || DEFAULT_LLM;
    } catch (reason) {
      if (loadVersion === targetLoadVersion && !destroyed) {
        error = `Could not prepare recording: ${String(reason)}`;
      }
    } finally {
      if (loadVersion === targetLoadVersion && !destroyed) loadingTarget = false;
    }
  }

  function backToPatients() {
    targetLoadVersion += 1;
    view = "patients";
    selectedPatient = null;
    sessions = [];
    loadingTarget = false;
    error = "";
  }

  async function beginRecording() {
    if (!selectedPatient || !canStart) return;
    starting = true;
    error = "";
    let createdSession: Session | null = null;
    try {
      const session = targetMode === "new"
        ? await invoke<Session>("create_session", {
            data: {
              patient_id: selectedPatient.id,
              date: sessionDate,
              start_time: sessionTime || null,
              title: sessionTitle.trim() || null,
              session_type: null,
            },
          })
        : selectedExistingSession;
      if (!session) throw new Error("Choose an existing session.");
      if (targetMode === "new") createdSession = session;

      await startRecording(
        {
          session_id: session.id,
          input_kind: recordingType,
          formats: targetMode === "new"
            ? selectedFormats
            : session.notes.map((note) => note.format),
          llm_model: defaultLlm,
          thinking: false,
          num_speakers: numSpeakers,
          created_session: targetMode === "new",
        },
        selectedInputDevice || undefined,
        isSessionRecording ? selectedOutputDevice || undefined : undefined,
      );

      activeSession = session;
      activePatientName = selectedPatient.name;
      savedNeedsReview = false;
      isRecording = true;
      isPaused = false;
      elapsed = 0;
      level = 0;
      view = "recording";
    } catch (reason) {
      error = `Could not start recording: ${String(reason)}`;
      if (createdSession) {
        try {
          await invoke("delete_session", { id: createdSession.id });
        } catch {
          // Keeping an empty session is safer than hiding a failed cleanup.
        }
      }
    } finally {
      starting = false;
    }
  }

  async function togglePause() {
    if (recordingAction !== null) return;
    const action = isPaused ? "resume" : "pause";
    recordingAction = "pause";
    error = "";
    try {
      if (isPaused) {
        await resumeRecording();
        isPaused = false;
      } else {
        await pauseRecording();
        isPaused = true;
        level = 0;
      }
    } catch (reason) {
      error = `Could not ${action}: ${String(reason)}`;
    } finally {
      recordingAction = null;
    }
  }

  async function stop() {
    if (recordingAction !== null) return;
    recordingAction = "stop";
    error = "";
    try {
      await stopRecording();
    } catch (reason) {
      error = `Could not stop recording: ${String(reason)}`;
    } finally {
      recordingAction = null;
    }
  }

  function startAnotherRecording() {
    targetLoadVersion += 1;
    view = "patients";
    selectedPatient = null;
    activeSession = null;
    activePatientName = "";
    patientSearch = "";
    savedNeedsReview = false;
    error = "";
  }
</script>

<svelte:head><title>Gist Menu Bar</title></svelte:head>
<svelte:window onkeydown={(event) => {
  if (event.key === "Escape") void closePopover();
}} />

<main class="menu-bar-shell">
  <section class="menu-bar-card" aria-label="Gist menu bar">
    <header class="menu-bar-header">
      <div class="brand-row">
        <span class="brand-mark" class:active={isRecording}></span>
        <strong>Gist</strong>
      </div>
      <button class="icon-action" type="button" aria-label="Close" onclick={closePopover}>
        <svg aria-hidden="true" viewBox="0 0 16 16">
          <path d="M3.5 3.5 12.5 12.5M12.5 3.5 3.5 12.5"></path>
        </svg>
      </button>
    </header>

    {#if error}
      <div class="popover-error" role="alert">{error}</div>
    {/if}

    {#if loading}
      <div class="loading-state">Loading Gist…</div>
    {:else if view === "recording"}
      <div class="recording-view">
        <div class="recording-status">
          <span class="recording-status-dot" class:paused={isPaused}></span>
          <span>{isPaused ? "Recording paused" : "Recording"}</span>
        </div>
        <div class="recording-time">{formatElapsed(elapsed)}</div>
        <div class="recording-target">
          <strong>{activePatientName || "Session"}</strong>
          {#if activeSession}<span>{formatSession(activeSession)}</span>{/if}
        </div>
        <div class="level-track" aria-label="Recording level">
          <div style:width={`${Math.min(level * 100, 100)}%`}></div>
        </div>
        <div class="recording-actions">
          <button class="secondary-action" type="button" disabled={recordingAction !== null} onclick={togglePause}>
            {recordingAction === "pause" ? "Working…" : isPaused ? "Resume" : "Pause"}
          </button>
          <button class="stop-action" type="button" disabled={recordingAction !== null} onclick={stop}>
            {recordingAction === "stop" ? "Stopping…" : "Stop recording"}
          </button>
        </div>
      </div>
    {:else if view === "saved"}
      <div class="saved-view">
        <div class="saved-icon">✓</div>
        <h2>Recording saved</h2>
        <p>{savedNeedsReview ? "This short recording needs review in Gist before it is processed." : "Gist is transcribing and processing it in the background."}</p>
        <button class="primary-action" type="button" onclick={openGist}>Open Gist</button>
        <button class="text-action" type="button" onclick={startAnotherRecording}>Start another recording</button>
      </div>
    {:else if view === "failed"}
      <div class="saved-view failed-view">
        <div class="saved-icon" aria-hidden="true">!</div>
        <h2>Recording stopped</h2>
        <p>The recording could not be completed. Open Gist to review the error and try again.</p>
        <button class="primary-action" type="button" onclick={openGist}>Open Gist</button>
        <button class="text-action" type="button" onclick={startAnotherRecording}>Try another recording</button>
      </div>
    {:else if view === "patients"}
      <div class="popover-heading">
        <h1>Start recording</h1>
        <p>Choose a patient.</p>
      </div>
      {#if patients.length === 0}
        <div class="empty-view">
          <p>Add a patient in Gist before starting a recording.</p>
          <button class="primary-action" type="button" onclick={openGist}>Open Gist</button>
        </div>
      {:else}
        <div class="patient-search">
          <input bind:value={patientSearch} placeholder="Search patients" aria-label="Search patients" />
        </div>
        <div class="patient-options">
          {#each filteredPatients as patient (patient.id)}
            <button type="button" onclick={() => selectPatient(patient)}>
              <span>{patient.name}</span><span aria-hidden="true">›</span>
            </button>
          {/each}
          {#if filteredPatients.length === 0}
            <p class="no-results">No matching patients.</p>
          {/if}
        </div>
      {/if}
    {:else}
      <div class="target-header">
        <button class="back-action" type="button" onclick={backToPatients} aria-label="Back">‹</button>
        <div><span>Recording for</span><strong>{selectedPatient?.name}</strong></div>
      </div>

      <div class="target-content">
        <div class="target-choice">
          <span class="section-label">Recording type</span>
          <div class="segmented-control" aria-label="Recording type">
            <button class:active={isSessionRecording} aria-pressed={isSessionRecording} type="button" onclick={() => recordingType = "session_transcript"}>Session recording</button>
            <button class:active={!isSessionRecording} aria-pressed={!isSessionRecording} type="button" onclick={() => recordingType = "clinician_note"}>Clinician note</button>
          </div>
        </div>

        <div class="target-choice">
          <span class="section-label">Destination</span>
          <div class="segmented-control" aria-label="Recording destination">
            <button class:active={targetMode === "new"} aria-pressed={targetMode === "new"} type="button" onclick={() => targetMode = "new"}>New session</button>
            <button class:active={targetMode === "existing"} aria-pressed={targetMode === "existing"} type="button" onclick={() => targetMode = "existing"}>Existing session</button>
          </div>
        </div>

        {#if loadingTarget}
          <div class="loading-state compact">Preparing recording…</div>
        {:else}
          {#if targetMode === "new"}
            <div class="metadata-grid">
              <label>Date<input type="date" bind:value={sessionDate} /></label>
              <label>Time<input type="time" bind:value={sessionTime} /></label>
            </div>
            <label class="field-label"><span class="field-label-heading">Title <span>optional</span></span><input bind:value={sessionTitle} placeholder="Session title" /></label>
          {:else}
            <div class="session-options">
              {#each sessions as session (session.id)}
                <label class:selected={selectedSessionId === session.id}>
                  <input type="radio" name="session" value={session.id} bind:group={selectedSessionId} />
                  <span class="session-option-copy">
                    <strong>{formatSessionTitle(session)}</strong>
                    <span>{formatSessionMeta(session)}</span>
                  </span>
                </label>
              {/each}
              {#if sessions.length === 0}<p class="no-results">No existing sessions.</p>{/if}
            </div>
          {/if}

          <details class="recording-options">
            <summary>Recording options</summary>
            <label class="field-label">Microphone
              <select bind:value={selectedInputDevice}>
                {#each inputDevices as device (device.id)}<option value={device.id}>{device.name}</option>{/each}
              </select>
            </label>
            {#if isSessionRecording}
              <label class="field-label">Computer audio
                <select bind:value={selectedOutputDevice}>
                  {#each outputDevices as device (device.id)}<option value={device.id}>{device.name}</option>{/each}
                </select>
              </label>
              <label class="field-label">Number of speakers
                <select bind:value={numSpeakers}>
                  {#each DIARIZATION_SPEAKER_COUNTS as count}<option value={count}>{count}</option>{/each}
                </select>
              </label>
            {/if}
          </details>

          {#if inputDevices.length === 0 || (isSessionRecording && outputDevices.length === 0)}
            <p class="device-warning">{isSessionRecording ? "A microphone and computer-audio device are required." : "A microphone is required."}</p>
          {/if}
          <button class="primary-action start-action" type="button" disabled={!canStart} onclick={beginRecording}>
            {starting ? "Starting…" : "Start recording"}
          </button>
        {/if}
      </div>
    {/if}

    {#if view !== "recording" && view !== "saved" && view !== "failed"}
      <footer><button class="text-action" type="button" onclick={openGist}>Open Gist</button></footer>
    {/if}
  </section>
</main>

<style>
  :global(html), :global(body) { width: 100%; background: transparent; }
  :global(body) { overflow: hidden; }
  .menu-bar-shell, .menu-bar-shell * { box-sizing: border-box; }
  button, input, select { max-width: 100%; font: inherit; }
  .menu-bar-shell { width: 100%; height: 100vh; overflow: hidden; padding: 8px; }
  .menu-bar-card { display: flex; width: 100%; min-width: 0; flex-direction: column; height: 100%; overflow: hidden; border: 1px solid var(--border); border-radius: 12px; background: var(--bg-sidebar); box-shadow: var(--shadow-md); }
  .menu-bar-header { display: flex; align-items: center; justify-content: space-between; min-height: 48px; padding: 0 14px; border-bottom: 1px solid var(--border-subtle); }
  .brand-row { display: flex; align-items: center; gap: 8px; font-size: 15px; }
  .brand-mark { width: 10px; height: 10px; border: 2px solid var(--accent); border-radius: 50%; }
  .brand-mark.active { border: 0; background: #e74c3c; box-shadow: 0 0 0 3px color-mix(in srgb, #e74c3c 16%, transparent); }
  .icon-action, .back-action, .text-action { border: 0; background: transparent; color: var(--text-muted); cursor: pointer; }
  .icon-action { display: grid; width: 30px; height: 30px; place-items: center; border: 1px solid transparent; border-radius: var(--radius-sm); line-height: 0; }
  .icon-action svg { display: block; width: 16px; height: 16px; }
  .icon-action path { fill: none; stroke: currentColor; stroke-width: 1.75; stroke-linecap: round; }
  .icon-action:hover, .back-action:hover, .text-action:hover { border-color: var(--border); color: var(--text); background: var(--bg-hover); }
  .popover-error { margin: 10px 14px 0; padding: 9px 10px; border-radius: 7px; background: color-mix(in srgb, var(--error) 10%, transparent); color: var(--error); font-size: 12px; }
  .popover-heading { padding: 18px 18px 12px; }
  .popover-heading h1 { margin: 0; font-size: 20px; }
  .popover-heading p { margin: 3px 0 0; color: var(--text-muted); font-size: 13px; }
  .patient-search { padding: 0 14px 10px; }
  .patient-search input, .field-label input, .field-label select, .metadata-grid input { width: 100%; }
  .patient-options, .session-options { flex: 1; overflow-y: auto; border-top: 1px solid var(--border-subtle); border-bottom: 1px solid var(--border-subtle); }
  .patient-options button { display: flex; width: 100%; align-items: center; justify-content: space-between; padding: 11px 16px; border: 0; border-bottom: 1px solid var(--border-subtle); background: transparent; color: var(--text); font-size: 13px; text-align: left; cursor: pointer; }
  .patient-options button:hover { background: var(--bg-hover); }
  .no-results, .device-warning { margin: 0; padding: 14px 16px; color: var(--text-muted); font-size: 12px; }
  .loading-state, .empty-view { display: grid; flex: 1; place-content: center; gap: 14px; padding: 28px; color: var(--text-muted); text-align: center; }
  .loading-state.compact { min-height: 180px; }
  .target-header { display: flex; align-items: center; gap: 8px; padding: 12px 14px; border-bottom: 1px solid var(--border-subtle); }
  .back-action { width: 30px; height: 30px; border: 1px solid transparent; border-radius: var(--radius-sm); font-size: 25px; line-height: 1; }
  .target-header div { display: flex; min-width: 0; flex-direction: column; }
  .target-header span { color: var(--text-muted); font-size: 11px; }
  .target-header strong { overflow: hidden; font-size: 15px; text-overflow: ellipsis; white-space: nowrap; }
  .target-content { min-width: 0; max-width: 100%; flex: 1; overflow-x: hidden; overflow-y: auto; padding: 14px; }
  .target-choice + .target-choice { margin-top: 12px; }
  .section-label { display: block; margin: 0 0 5px 2px; color: var(--text-muted); font-size: 11px; font-weight: 600; }
  .segmented-control { display: grid; grid-template-columns: 1fr 1fr; padding: 3px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--bg-sidebar); }
  .segmented-control button { min-width: 0; padding: 7px 6px; border: 0; border-radius: 4px; background: transparent; color: var(--text-muted); font-size: 12px; font-weight: 500; cursor: pointer; }
  .segmented-control button:hover { color: var(--text); }
  .segmented-control button.active { background: var(--accent-light); color: var(--text); font-weight: 600; }
  .metadata-grid { display: grid; min-width: 0; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 10px; margin-top: 14px; }
  .metadata-grid label, .field-label { display: grid; gap: 5px; color: var(--text-muted); font-size: 12px; font-weight: 600; }
  .metadata-grid label, .field-label, .recording-options { min-width: 0; }
  .field-label input, .field-label select, .metadata-grid input { min-width: 0; }
  .field-label { margin-top: 10px; }
  .field-label-heading { display: flex; align-items: baseline; gap: 4px; font-weight: 600; }
  .field-label-heading span { font-weight: 400; }
  .session-options { width: 100%; min-width: 0; max-height: 190px; overflow-x: hidden; margin-top: 12px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--bg-sidebar); }
  .session-options label { display: grid; min-width: 0; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 9px; padding: 10px 11px; border-bottom: 1px solid var(--border-subtle); color: var(--text-muted); font-size: 12px; cursor: pointer; }
  .session-options input[type="radio"] { width: auto; min-width: auto; flex: 0 0 auto; margin: 0; }
  .session-option-copy { display: flex; min-width: 0; flex-direction: column; gap: 2px; }
  .session-option-copy strong { overflow: hidden; color: var(--text); font-size: 13px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .session-option-copy > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-options label.selected { background: var(--accent-light); color: var(--text); }
  .recording-options { width: 100%; max-width: 100%; overflow-x: hidden; margin-top: 12px; padding: 10px 11px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--bg-sidebar); }
  .recording-options summary { color: var(--text-muted); font-size: 12px; font-weight: 600; cursor: pointer; }
  .primary-action, .secondary-action, .stop-action { min-height: var(--control-height); border: 1px solid var(--border); border-radius: 7px; padding: 0 14px; font-size: 13px; font-weight: 600; cursor: pointer; transition: background .1s, border-color .1s; }
  .primary-action { border-color: var(--accent); background: var(--accent); color: var(--color-surface); }
  .primary-action:hover:not(:disabled) { border-color: var(--accent-hover); background: var(--accent-hover); }
  .primary-action:disabled { opacity: .45; cursor: default; }
  .secondary-action:disabled, .stop-action:disabled { opacity: .55; cursor: default; }
  .start-action { width: 100%; margin-top: 14px; }
  footer { display: flex; justify-content: center; min-height: 42px; padding: 6px; border-top: 1px solid var(--border-subtle); }
  footer .text-action { padding: 5px 10px; border-radius: 6px; font-size: 12px; }
  .recording-view, .saved-view { display: flex; flex: 1; flex-direction: column; align-items: center; justify-content: center; padding: 24px; text-align: center; }
  .recording-status { display: flex; align-items: center; gap: 8px; color: var(--text-muted); font-size: 13px; font-weight: 600; }
  .recording-status-dot { width: 10px; height: 10px; border-radius: 50%; background: #e74c3c; }
  .recording-status-dot.paused { background: var(--warning); }
  .recording-time { margin-top: 14px; font-variant-numeric: tabular-nums; font-size: 48px; font-weight: 650; letter-spacing: -2px; }
  .recording-target { display: flex; max-width: 100%; flex-direction: column; margin-top: 10px; }
  .recording-target span { overflow: hidden; color: var(--text-muted); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
  .level-track { width: 100%; height: 5px; margin-top: 24px; overflow: hidden; border-radius: 4px; background: var(--bg-subtle); }
  .level-track div { height: 100%; border-radius: inherit; background: var(--accent); transition: width .12s linear; }
  .recording-actions { display: grid; width: 100%; grid-template-columns: 1fr 1.35fr; gap: 10px; margin-top: 18px; }
  .secondary-action { background: var(--bg-sidebar); color: var(--text); }
  .secondary-action:hover { background: var(--bg-hover); }
  .stop-action { border-color: #c0392b; background: #e74c3c; color: white; }
  .stop-action:hover { background: #c0392b; }
  .saved-icon { display: grid; width: 44px; height: 44px; place-items: center; border-radius: 50%; background: var(--accent-light); color: var(--accent); font-size: 22px; font-weight: 700; }
  .failed-view .saved-icon { background: color-mix(in srgb, var(--error) 10%, transparent); color: var(--error); }
  .saved-view h2 { margin-top: 14px; font-size: 20px; }
  .saved-view p { max-width: 260px; margin: 5px 0 18px; color: var(--text-muted); font-size: 13px; }
  .saved-view .text-action { margin-top: 8px; padding: 6px 10px; border-radius: 6px; }
</style>

<script lang="ts">
  import "../app.css";
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    cancelSidecar,
    checkIsRecording,
    getRecordingState,
    getRecordingJob,
    isRunning,
    listRecoverableRecordingJobs,
    onProgress,
    onRecordingStopped,
    onRecordingError,
    onRecordingTick,
    pauseRecording,
    resumeRecording,
    startSidecar,
    stopRecording,
    discardRecordingJob,
    setRecordingJobError,
  } from "$lib/rpc";
  import { processSessionFromAudio } from "$lib/processSession";
  import {
    activeOperation,
    appearance,
    currentOperation,
    darkMode,
    isRecording,
    patients,
    progressBase,
    progressEta,
    progressPercent,
    progressScale,
    progressStage,
    recordingContext,
    recordingElapsed,
    recordingLevel,
    recordingJobsProcessing,
    recordingPaused,
    selectedPatientId,
    sessionUpdate,
    sidecarBusy,
    sidecarRunning,
  } from "$lib/stores";
  import { get } from "svelte/store";
  import { loadAppearance } from "$lib/settings";
  import { page } from "$app/stores";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import type { Patient, RecordingJob, Session } from "$lib/types";
  import { confirmRegenerateAttachedNotes } from "$lib/confirmations";
  import { deferFeedbackPrompt, dismissFeedbackPrompt, recordAppLaunch } from "$lib/feedback";
  import FeedbackPrompt from "$lib/components/FeedbackPrompt.svelte";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import UpdatePrompt from "$lib/components/UpdatePrompt.svelte";

  let { children } = $props();
  let onboardingComplete = $state(false);
  let feedbackPromptPending = $state(false);
  let showFeedbackPrompt = $state(false);
  let launchCount = 0;
  let availableUpdate = $state<Update | null>(null);
  let dismissedUpdateVersion = $state<string | null>(null);

  let unlistenProgress: UnlistenFn | null = null;
  let unlistenState: UnlistenFn | null = null;
  let unlistenRecTick: UnlistenFn | null = null;
  let unlistenRecStopped: UnlistenFn | null = null;
  let unlistenRecError: UnlistenFn | null = null;
  let recordingPollInterval: ReturnType<typeof setInterval> | null = null;
  let updateCheckTimeout: ReturnType<typeof setTimeout> | null = null;
  let updateCheckInterval: ReturnType<typeof setInterval> | null = null;
  let layoutDestroyed = false;
  let themeMediaQuery: MediaQueryList | null = null;
  let handleSystemThemeChange: (() => void) | null = null;
  let showAddForm = $state(false);
  let newName = $state("");
  let addError = $state("");
  let recoverableRecordingJobs = $state<RecordingJob[]>([]);
  let recordingRecoveryError = $state("");
  const processingRecordingJobs = new Set<string>();
  let patientSearch = $state("");
  let patientSearchInput = $state<HTMLInputElement | null>(null);
  let filteredPatients = $derived(
    $patients.filter((patient) =>
      patient.name.toLocaleLowerCase().includes(patientSearch.trim().toLocaleLowerCase())
    )
  );

  function focusPatient(index: number) {
    const items = [...document.querySelectorAll<HTMLAnchorElement>(".patient-item")];
    if (items.length === 0) return;
    items[Math.max(0, Math.min(index, items.length - 1))]?.focus();
  }

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

  function sanitizeProgressStage(stage: string, operationType: string | null): string {
    const normalized = stage.trim().toLowerCase();

    if (normalized.startsWith("loading speaker")) return "Preparing speaker identification...";
    if (normalized.includes("speaker diarization complete") || normalized.includes("finalizing speaker turns")) {
      return "Finalizing transcript...";
    }
    if (normalized.startsWith("loading ")) {
      if (operationType === "generate_note") return "Preparing note generation...";
      if (operationType === "download_model") return "Preparing model download...";
      return "Preparing transcription...";
    }
    if (normalized === "transcribing") return "Transcribing...";
    if (normalized === "generating") return "Generating note...";
    if (normalized === "done") return "Finalizing note...";
    if (normalized.startsWith("starting download")) return "Preparing model download...";
    if (normalized.startsWith("downloading ")) return "Downloading model files...";
    if (normalized === "download complete") return "Model download complete";
    if (normalized.includes("speaker diarization") || normalized.includes("speaker analysis")) {
      return "Preparing speaker identification...";
    }
    if (normalized.includes("analyzing speech segments")) return "Analyzing speech...";
    if (normalized.includes("estimating speakers")) return "Estimating number of speakers...";
    if (normalized.includes("identifying speakers")) return "Identifying speakers...";
    return stage;
  }

  function contextForRecordingJob(job: RecordingJob, session: Session): import("$lib/processSession").RecordingContext {
    return {
      patientId: session.patient_id,
      formats: job.formats,
      defaultLlm: job.llm_model,
      thinking: job.thinking,
      inputKind: job.input_kind as import("$lib/types").SessionInputKind,
      diarize: job.diarize,
      session,
      isNewSession: job.created_session,
      jobId: job.id,
    };
  }

  async function processRecordingJob(job: RecordingJob) {
    if (processingRecordingJobs.has(job.id)) return;
    processingRecordingJobs.add(job.id);
    recordingJobsProcessing.set(true);
    try {
      const session = await invoke<Session | null>("get_session", { id: job.session_id });
      if (!session) throw new Error("The session for this recording is no longer available.");
      const regenerateExisting = job.created_session
        ? false
        : await confirmRegenerateAttachedNotes(session.notes.length);
      const processedSession = await processSessionFromAudio(
        job.audio_file,
        { ...contextForRecordingJob(job, session), regenerateExisting },
      );
      sessionUpdate.set(processedSession);
      recoverableRecordingJobs = recoverableRecordingJobs.filter((candidate) => candidate.id !== job.id);
    } finally {
      processingRecordingJobs.delete(job.id);
      recordingJobsProcessing.set(processingRecordingJobs.size > 0);
    }
  }

  async function handleRecordingStopped(data: { job_id: string; file_path: string; duration_seconds: number; is_short_recording: boolean }) {
    isRecording.set(false);
    recordingPaused.set(false);
    recordingElapsed.set(0);
    recordingLevel.set(0);
    recordingContext.set(null);

    try {
      const job = await getRecordingJob(data.job_id);
      if (data.is_short_recording) {
        recordingRecoveryError = "This recording is shorter than five seconds. It was saved, but may not contain enough audio to transcribe. Process it only if that was intentional, or discard it.";
        recoverableRecordingJobs = [job, ...recoverableRecordingJobs.filter((candidate) => candidate.id !== job.id)];
        return;
      }
      await processRecordingJob(job);
    } catch (e) {
      const message = `Gist saved the recording, but could not process it: ${String(e)}`;
      recordingRecoveryError = message;
      try {
        await setRecordingJobError(data.job_id, message);
        recoverableRecordingJobs = await listRecoverableRecordingJobs();
      } catch {
        // The file remains in the managed recordings folder even if its status cannot be updated.
      }
    }
  }

  async function handleRecordingError(data: { message: string }) {
    isRecording.set(false);
    recordingPaused.set(false);
    recordingRecoveryError = `${data.message} Gist stopped recording to protect the saved audio.`;
    try {
      await stopRecording();
    } catch {
      // The partial recording remains recoverable through the durable job.
    }
    try {
      recoverableRecordingJobs = await listRecoverableRecordingJobs();
    } catch {
      // Keep the actionable error even if recovery discovery also fails.
    }
  }

  async function retryRecordingJob(job: RecordingJob) {
    recordingRecoveryError = "";
    try {
      await processRecordingJob(job);
    } catch (e) {
      const message = `Could not process this recording: ${String(e)}`;
      recordingRecoveryError = message;
      await setRecordingJobError(job.id, message);
      recoverableRecordingJobs = await listRecoverableRecordingJobs();
    }
  }

  async function discardRecoveredRecording(job: RecordingJob) {
    recordingRecoveryError = "";
    try {
      await discardRecordingJob(job.id);
      recoverableRecordingJobs = recoverableRecordingJobs.filter((candidate) => candidate.id !== job.id);
    } catch (e) {
      recordingRecoveryError = `Could not discard this recording: ${String(e)}`;
    }
  }

  async function remindAboutFeedbackLater() {
    feedbackPromptPending = false;
    showFeedbackPrompt = false;
    try {
      await deferFeedbackPrompt(launchCount);
    } catch (e) {
      console.error("Could not save feedback reminder:", e);
    }
  }

  async function stopAskingForFeedback() {
    feedbackPromptPending = false;
    showFeedbackPrompt = false;
    try {
      await dismissFeedbackPrompt();
    } catch (e) {
      console.error("Could not save feedback preference:", e);
    }
  }

  async function checkForUpdates() {
    if (layoutDestroyed || availableUpdate) return;
    try {
      const update = await check({ timeout: 10_000 });
      if (!update || layoutDestroyed) {
        await update?.close();
        return;
      }
      if (update.version === dismissedUpdateVersion) {
        await update.close();
        return;
      }
      availableUpdate = update;
    } catch (e) {
      // Update checks are best-effort. Offline use and GitHub outages should
      // never affect recording, transcription, or local note storage.
      console.debug("Application update check unavailable:", e);
    }
  }

  async function dismissUpdate() {
    const update = availableUpdate;
    if (!update) return;
    dismissedUpdateVersion = update.version;
    availableUpdate = null;
    try {
      await update.close();
    } catch (e) {
      console.debug("Could not close update handle:", e);
    }
  }

  onMount(async () => {
    try {
      const launch = await recordAppLaunch();
      launchCount = launch.launchCount;
      feedbackPromptPending = launch.shouldPrompt;
    } catch (e) {
      console.error("Could not record app launch:", e);
    }

    // Install this before any awaited startup work so a just-stopped recording
    // is always routed through its durable job.
    unlistenRecStopped = await onRecordingStopped(handleRecordingStopped);
    if (layoutDestroyed) {
      unlistenRecStopped();
      return;
    }
    unlistenRecError = await onRecordingError(handleRecordingError);
    if (layoutDestroyed) {
      unlistenRecError();
      return;
    }

    // Auto-start sidecar silently
    try {
      const running = await isRunning();
      if (running) {
        sidecarRunning.set(true);
      } else {
        await startSidecar();
        sidecarRunning.set(true);
      }
    } catch (e) {
      console.error("Failed to auto-start sidecar:", e);
    }

    // Load patients
    try {
      const list = await invoke<Patient[]>("list_patients");
      patients.set(list);
    } catch (e) {
      console.error("Failed to load patients:", e);
    }

    // Global progress listener
    unlistenProgress = await onProgress((data) => {
      const base = get(progressBase);
      const scale = get(progressScale);
      const operation = get(activeOperation);
      const safeStage = sanitizeProgressStage(data.stage, operation.type);
      progressPercent.set(base + Math.round((data.percent / 100) * scale));
      progressStage.set(safeStage);
      progressEta.set(data.eta_seconds ?? null);
      if (
        operation.type === "transcribe" &&
        safeStage !== "Preparing transcription..." &&
        safeStage !== "Transcribing..."
      ) {
        activeOperation.set({ ...operation, label: safeStage });
      }
    });
    if (layoutDestroyed) {
      unlistenProgress();
      return;
    }

    // Sidecar busy state listener
    unlistenState = await listen<{ busy: boolean }>("sidecar-state", (event) => {
      sidecarBusy.set(event.payload.busy);
      if (!event.payload.busy) {
        activeOperation.set({ type: null, label: "" });
        progressPercent.set(0);
        progressStage.set("");
        progressEta.set(null);
        progressBase.set(0);
        progressScale.set(100);
      }
    });
    if (layoutDestroyed) {
      unlistenState();
      return;
    }

    // Load dark mode setting
    await loadAppearance();
    if (layoutDestroyed) return;
    themeMediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    handleSystemThemeChange = () => {
      if (get(appearance) !== "system") return;
      darkMode.set(themeMediaQuery?.matches ?? false);
      document.documentElement.classList.toggle("dark", themeMediaQuery?.matches ?? false);
    };
    themeMediaQuery.addEventListener("change", handleSystemThemeChange);

    // Recording state sync — recover state after navigation/reload
    try {
      const state = await getRecordingState();
      if (state.is_recording) {
        isRecording.set(true);
        recordingPaused.set(state.is_paused);
        recordingElapsed.set(state.elapsed_seconds);
      }
    } catch (e) {
      console.error("Failed to sync recording state:", e);
    }

    // Recording tick listener
    unlistenRecTick = await onRecordingTick((data) => {
      recordingElapsed.set(data.elapsed_seconds);
      recordingPaused.set(data.is_paused);
      recordingLevel.set(data.level);
    });
    if (layoutDestroyed) {
      unlistenRecTick();
      return;
    }

    try {
      recoverableRecordingJobs = await listRecoverableRecordingJobs();
    } catch (e) {
      recordingRecoveryError = `Could not check for interrupted recordings: ${String(e)}`;
    }

    // Fallback polling (every 1s) in case events are missed
    recordingPollInterval = setInterval(async () => {
      try {
        const running = await checkIsRecording();
        if (running !== $isRecording) {
          isRecording.set(running);
        }
        if (running) {
          const state = await getRecordingState();
          recordingPaused.set(state.is_paused);
          recordingElapsed.set(state.elapsed_seconds);
        } else {
          recordingPaused.set(false);
        }
      } catch {}
    }, 1000);
    if (layoutDestroyed && recordingPollInterval) {
      clearInterval(recordingPollInterval);
      recordingPollInterval = null;
    }

    // Check in the background after startup, then periodically while the app
    // is open. This remains silent when offline or when no release is present.
    updateCheckTimeout = setTimeout(() => void checkForUpdates(), 5_000);
    updateCheckInterval = setInterval(() => void checkForUpdates(), 6 * 60 * 60 * 1_000);
  });

  $effect(() => {
    if (feedbackPromptPending && onboardingComplete) {
      showFeedbackPrompt = true;
    }
  });

  // Apply dark mode class to <html>
  $effect(() => {
    const systemDark = typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches;
    const useDark = $appearance === "dark" || ($appearance === "system" && systemDark);
    darkMode.set(useDark);
    if (useDark) {
      document.documentElement.classList.add("dark");
    } else {
      document.documentElement.classList.remove("dark");
    }
  });

  onDestroy(() => {
    layoutDestroyed = true;
    unlistenProgress?.();
    unlistenState?.();
    unlistenRecTick?.();
    unlistenRecStopped?.();
    unlistenRecError?.();
    if (recordingPollInterval) clearInterval(recordingPollInterval);
    if (updateCheckTimeout) clearTimeout(updateCheckTimeout);
    if (updateCheckInterval) clearInterval(updateCheckInterval);
    if (availableUpdate) void availableUpdate.close();
    if (themeMediaQuery && handleSystemThemeChange) {
      themeMediaQuery.removeEventListener("change", handleSystemThemeChange);
    }
  });

  // Sync selectedPatientId from URL
  $effect(() => {
    const p = $page.url.pathname;
    const match = p.match(/^\/patients\/(.+)$/);
    selectedPatientId.set(match ? match[1] : null);
  });

  async function addPatient() {
    if (!newName.trim()) return;
    addError = "";
    try {
      const created = await invoke<Patient>("create_patient", { data: { name: newName.trim() } });
      patients.update((list) => [created, ...list]);
      newName = "";
      showAddForm = false;
    } catch (e) {
      addError = String(e);
    }
  }

  async function handleCancel() {
    try {
      await cancelSidecar();
    } catch (e) {
      console.error("Cancel failed:", e);
    }
  }

  async function handleStopRecording() {
    try {
      await stopRecording();
    } catch (e) {
      console.error("Stop recording failed:", e);
    }
  }

  async function handleToggleRecordingPause() {
    try {
      if ($recordingPaused) {
        await resumeRecording();
        recordingPaused.set(false);
      } else {
        await pauseRecording();
        recordingPaused.set(true);
        recordingLevel.set(0);
      }
    } catch (e) {
      console.error("Pause/resume recording failed:", e);
    }
  }

  function formatElapsed(seconds: number): string {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = Math.floor(seconds % 60);
    if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  let pathname = $derived($page.url.pathname);
  const isSettings = $derived(pathname === "/settings");
  const isTemplates = $derived(pathname === "/templates");
  let lastWindowMouseDownAt = 0;

  function startWindowDrag(event: MouseEvent) {
    if (event.button !== 0) return;
    void getCurrentWindow().startDragging().catch((error) => {
      console.error("Failed to start window drag:", error);
    });
  }

  function handleWindowMouseDown(event: MouseEvent) {
    if (event.button !== 0) return;

    const now = Date.now();
    const isDoubleClick = event.detail >= 2 || now - lastWindowMouseDownAt < 400;
    lastWindowMouseDownAt = now;

    if (isDoubleClick) {
      event.preventDefault();
      toggleWindowMaximize();
      return;
    }

    startWindowDrag(event);
  }

  function toggleWindowMaximize() {
    void getCurrentWindow().toggleMaximize().catch((error) => {
      console.error("Failed to toggle window maximize:", error);
    });
  }
</script>

<div class="app-shell">
  <div
    class="window-drag-region"
    onmousedown={handleWindowMouseDown}
    role="presentation"
  ></div>

  <aside class="sidebar">
    <div class="sidebar-header">
      <h1>Gist</h1>
    </div>

    <div class="sidebar-section-label">Patients</div>

    <div class="sidebar-search">
      <svg class="sidebar-search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
        <circle cx="11" cy="11" r="7"></circle>
        <path d="m20 20-4-4"></path>
      </svg>
      <input
        bind:this={patientSearchInput}
        bind:value={patientSearch}
        placeholder="Search patients"
        aria-label="Search patients by name"
        onkeydown={(event) => {
          if (event.key === "Escape") patientSearch = "";
          if (event.key === "ArrowDown") {
            event.preventDefault();
            focusPatient(0);
          }
        }}
      />
      {#if patientSearch}
        <button class="sidebar-search-clear" type="button" onclick={() => patientSearch = ""} aria-label="Clear patient search">×</button>
      {/if}
    </div>

    <div class="patient-list">
      {#each filteredPatients as patient, index (patient.id)}
        <a
          href="/patients/{patient.id}"
          class="patient-item"
          class:active={$selectedPatientId === patient.id}
          onkeydown={(event) => {
            if (event.key === "ArrowDown") { event.preventDefault(); focusPatient(index + 1); }
            if (event.key === "ArrowUp") { event.preventDefault(); index === 0 ? patientSearchInput?.focus() : focusPatient(index - 1); }
            if (event.key === "Home") { event.preventDefault(); focusPatient(0); }
            if (event.key === "End") { event.preventDefault(); focusPatient(filteredPatients.length - 1); }
            if (event.key === "Escape") patientSearchInput?.focus();
          }}
        >
          <span class="patient-name">{patient.name}</span>
        </a>
      {/each}
      {#if $patients.length === 0}
        <div class="sidebar-empty">No patients yet.</div>
      {:else if filteredPatients.length === 0}
        <div class="sidebar-empty">No patients match <strong>“{patientSearch}”</strong>.</div>
      {/if}
    </div>

    <div class="sidebar-footer">
      {#if showAddForm}
        <div class="add-patient-form">
          <input
            bind:value={newName}
            placeholder="Patient name"
            onkeydown={(e) => {
              if (e.key === "Enter") addPatient();
              if (e.key === "Escape") { showAddForm = false; newName = ""; }
            }}
          />
          {#if addError}
            <p style="color: var(--error); font-size: 11px; margin-top: 4px;">{addError}</p>
          {/if}
          <div class="form-actions">
            <button class="btn btn-sm btn-primary" onclick={addPatient} disabled={!newName.trim()}>Add</button>
            <button class="btn btn-sm" onclick={() => { showAddForm = false; newName = ""; }}>Cancel</button>
          </div>
        </div>
      {:else}
        <button class="add-patient-btn" onclick={() => showAddForm = true}>
          <span>+</span>
          <span>Add Patient</span>
        </button>
      {/if}

      <a href="/templates" class="footer-link" class:active={isTemplates}>Note templates</a>
      <a href="/settings" class="footer-link" class:active={isSettings}>Settings</a>
    </div>
  </aside>

  <main class="main-content">
    {@render children()}
  </main>
</div>

{#if !onboardingComplete}
  <Onboarding onComplete={() => (onboardingComplete = true)} />
{/if}

{#if showFeedbackPrompt}
  <FeedbackPrompt
    onFeedbackAction={stopAskingForFeedback}
    onRemindLater={remindAboutFeedbackLater}
    onDontAskAgain={stopAskingForFeedback}
  />
{/if}

{#if availableUpdate && onboardingComplete}
  <UpdatePrompt
    update={availableUpdate}
    isBusy={$isRecording || $sidecarBusy || $recordingJobsProcessing || $currentOperation !== null}
    onDismiss={dismissUpdate}
  />
{/if}

{#if $sidecarBusy}
  <div class="progress-card" role="status" aria-live="polite">
    <div class="progress-card-header">
      <span class="progress-card-title">
        {$activeOperation.label || $progressStage || "Processing..."}
      </span>
      <button class="progress-card-cancel" onclick={handleCancel}>Cancel</button>
    </div>
    <div class="progress-bar">
      <div class="progress-bar-fill" style="width: {$progressPercent}%;"></div>
    </div>
    {#if $progressEta != null && $progressEta > 0}
      <div class="progress-card-eta">~{formatEta($progressEta)} remaining</div>
    {/if}
  </div>
{/if}

{#if $isRecording}
  <div class="recording-card">
    <div class="recording-card-header">
      <span class="recording-card-title">
        <span class="recording-dot" class:paused={$recordingPaused}></span>
        {$recordingPaused ? "Paused" : "REC"} {formatElapsed($recordingElapsed)}
      </span>
      <div class="recording-card-actions">
        <button class="recording-card-pause" onclick={handleToggleRecordingPause}>
          {$recordingPaused ? "Resume" : "Pause"}
        </button>
        <button class="recording-card-stop" onclick={handleStopRecording}>Stop</button>
      </div>
    </div>
    <div class="recording-level-bar">
      <div class="recording-level-fill" style="width: {Math.min($recordingLevel * 100, 100)}%;"></div>
    </div>
  </div>
{/if}

{#if recordingRecoveryError || recoverableRecordingJobs.length > 0}
  <section class="recording-recovery-card" role="alert" aria-live="assertive">
    <div>
      <strong>Recording recovery needed</strong>
      <p>{recordingRecoveryError || "Gist found a recording that was not fully processed. Your audio is kept locally until you choose what to do."}</p>
    </div>
    {#each recoverableRecordingJobs as job (job.id)}
      <div class="recording-recovery-actions">
        <span>{job.error || "Saved recording awaiting processing"}</span>
        {#if job.state !== "failed"}
          <button class="btn btn-sm btn-primary" onclick={() => retryRecordingJob(job)}>Process recording</button>
        {/if}
        <button class="btn btn-sm btn-danger" onclick={() => discardRecoveredRecording(job)}>Discard recording</button>
      </div>
    {/each}
  </section>
{/if}

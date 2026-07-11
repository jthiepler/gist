<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    createSessionInput,
    getPatientFormats,
    listAudioDevices,
    listNoteFormats,
    pauseRecording,
    resumeRecording,
    setPatientFormats,
    startRecording,
    stopRecording,
    transcribe,
    type AudioDevice,
  } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import type { RecordingContext } from "$lib/processSession";
  import {
    SESSION_INPUT_LABELS,
    SESSION_INPUT_SOURCES,
  } from "$lib/sessionInputs";
  import { generateSessionDocumentation } from "$lib/documentation";
  import {
    activeOperation,
    currentOperation,
    isRecording,
    progressBase,
    progressPercent,
    progressScale,
    progressStage,
    recordingContext,
    recordingElapsed,
    recordingLevel,
    recordingPaused,
    sidecarBusy,
  } from "$lib/stores";
  import { loadSettings } from "$lib/settings";
  import type { NoteFormatTemplate, Session, SessionInputKind } from "$lib/types";

  let {
    patientId,
    onComplete,
    onProcessingStart,
    onCancel,
  }: {
    patientId: string;
    onComplete: (session: Session) => void;
    onProcessingStart: (session: Session) => void;
    onCancel: () => void;
  } = $props();

  type InputMethod = "audio_file" | "recording" | "text" | "dictation";
  type SessionStartOption =
    | "record_session"
    | "upload_recording"
    | "paste_transcript"
    | "type_note"
    | "dictate_note";

  let sourceKind = $state<SessionInputKind>("session_transcript");
  let inputMethod = $state<InputMethod>("audio_file");
  let diarizeSession = $state(false);
  let audioPath = $state("");
  let textDraft = $state("");
  let formats = $state<NoteFormatTemplate[]>([]);
  let selectedFormats = $state<Set<string>>(new Set());
  let formatsLoaded = $state(false);
  let error = $state("");
  let phase = $state<"idle" | "transcribing" | "generating">("idle");

  let defaultLlm = $state("");
  let thinking = $state(false);

  let audioDevices = $state<AudioDevice[]>([]);
  let inputDevices = $state<AudioDevice[]>([]);
  let outputDevices = $state<AudioDevice[]>([]);
  let selectedInputDevice = $state("");
  let selectedOutputDevice = $state("");

  const opId = "new-session";

  let visibleFormats = $derived(
    formats.filter((f) => !f.hidden).sort((a, b) => a.name.localeCompare(b.name))
  );
  let selectedStartOption = $derived.by<SessionStartOption>(() => {
    if (sourceKind === "session_transcript" && inputMethod === "recording") return "record_session";
    if (sourceKind === "session_transcript" && inputMethod === "audio_file") return "upload_recording";
    if (sourceKind === "session_transcript" && inputMethod === "text") return "paste_transcript";
    if (sourceKind === "clinician_note" && inputMethod === "dictation") return "dictate_note";
    return "type_note";
  });
  let recordingLabel = $derived("Recording");
  let canSubmitDirectly = $derived(inputMethod === "audio_file" || inputMethod === "text");
  let sourceLabel = $derived(SESSION_INPUT_LABELS[sourceKind]);
  let textLabel = $derived(sourceKind === "session_transcript" ? "Session transcript" : "Clinician note");
  let textPlaceholder = $derived(
    sourceKind === "session_transcript"
      ? "Paste the session transcript here..."
      : "Type your post-session summary, observations, corrections, or plan details..."
  );
  let primaryActionLabel = $derived.by(() => {
    if (phase === "transcribing") return "Transcribing...";
    if (phase === "generating") return "Generating notes...";
    if (inputMethod === "audio_file") return "Transcribe and generate notes";
    if (inputMethod === "text") return "Save and generate notes";
    return "Start";
  });

  function toggleFormat(name: string) {
    const next = new Set(selectedFormats);
    if (next.has(name)) {
      next.delete(name);
    } else {
      next.add(name);
    }
    selectedFormats = next;
  }

  function formatTimer(seconds: number): string {
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  function finishOperation() {
    progressPercent.set(0);
    progressStage.set("");
    activeOperation.set({ type: null, label: "" });
    currentOperation.set(null);
    progressBase.set(0);
    progressScale.set(100);
    phase = "idle";
  }

  $effect(() => {
    (async () => {
      const ok = await ensureSidecar();
      if (!ok) return;
      try {
        formats = await listNoteFormats();
        const saved = await getPatientFormats(patientId);
        const visibleNames = formats.filter((f) => !f.hidden).map((f) => f.name);
        if (saved.length > 0) {
          const valid = saved.filter((n) => visibleNames.includes(n));
          selectedFormats = new Set(
            valid.length > 0 ? valid : ([visibleFormats[0]?.name].filter(Boolean) as string[])
          );
        } else {
          const first = visibleFormats[0];
          if (first) selectedFormats = new Set([first.name]);
        }
      } catch (e) {
        console.error("Failed to load formats/patient formats:", e);
      }
      formatsLoaded = true;

      const s = await loadSettings();
      if (s.defaultLlm) defaultLlm = s.defaultLlm;
      thinking = s.thinking;
    })();
  });

  $effect(() => {
    return () => {
      if ($currentOperation === opId) {
        currentOperation.set(null);
      }
    };
  });

  function selectStartOption(option: SessionStartOption) {
    if (phase !== "idle" || $isRecording) return;
    if (option === "record_session") {
      sourceKind = "session_transcript";
      inputMethod = "recording";
    } else if (option === "upload_recording") {
      sourceKind = "session_transcript";
      inputMethod = "audio_file";
    } else if (option === "paste_transcript") {
      sourceKind = "session_transcript";
      inputMethod = "text";
    } else if (option === "dictate_note") {
      sourceKind = "clinician_note";
      inputMethod = "dictation";
    } else {
      sourceKind = "clinician_note";
      inputMethod = "text";
    }
    if (sourceKind !== "session_transcript") diarizeSession = false;
    error = "";
    if ((inputMethod === "recording" || inputMethod === "dictation") && audioDevices.length === 0) {
      loadAudioDevices();
    }
  }

  $effect(() => {
    if ((inputMethod === "recording" || inputMethod === "dictation") && audioDevices.length === 0) {
      loadAudioDevices();
    }
  });

  $effect(() => {
    const ctx = $recordingContext;
    if (ctx && ctx.patientId === patientId && $isRecording) {
      sourceKind = ctx.inputKind;
      inputMethod = ctx.inputKind === "clinician_note" ? "dictation" : "recording";
      diarizeSession = ctx.diarize ?? false;
    }
  });

  async function loadAudioDevices() {
    try {
      const devices = await listAudioDevices();
      audioDevices = devices;
      inputDevices = devices.filter((d) => d.device_type === "input");
      outputDevices = devices.filter((d) => d.device_type === "output");
      if (inputDevices.length > 0 && !selectedInputDevice) {
        selectedInputDevice = inputDevices[0].name;
      }
      if (outputDevices.length > 0 && !selectedOutputDevice) {
        selectedOutputDevice = outputDevices[0].name;
      }
    } catch (e) {
      console.error("Failed to load audio devices:", e);
    }
  }

  async function handleStartRecording() {
    error = "";
    try {
      const sortedFormats = [...selectedFormats].sort((a, b) => a.localeCompare(b));
      const ctx: RecordingContext = {
        patientId,
        formats: sortedFormats,
        defaultLlm,
        thinking,
        inputKind: sourceKind,
        diarize: sourceKind === "session_transcript" && diarizeSession,
      };
      recordingContext.set(ctx);
      await startRecording(
        selectedInputDevice || undefined,
        inputMethod === "recording" ? selectedOutputDevice || undefined : undefined
      );
      isRecording.set(true);
      recordingPaused.set(false);
      recordingElapsed.set(0);
    } catch (e) {
      error = `Failed to start recording: ${e}`;
      recordingContext.set(null);
    }
  }

  async function handleStopRecording() {
    try {
      await stopRecording();
    } catch (e) {
      error = `Failed to stop recording: ${e}`;
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
      error = `Failed to ${$recordingPaused ? "resume" : "pause"} recording: ${e}`;
    }
  }

  async function pickFile() {
    try {
      const path = await invoke<string | null>("pick_audio_file");
      if (path) audioPath = path;
    } catch (e) {
      error = String(e);
    }
  }

  async function start() {
    const startsFromText = inputMethod === "text";
    const startsFromAudioFile = inputMethod === "audio_file";
    const cleanedText = textDraft.trim();

    if (startsFromAudioFile && !audioPath) {
      error = "Please select a session recording.";
      return;
    }
    if (startsFromText && !cleanedText) {
      error =
        sourceKind === "session_transcript"
          ? "Please paste a session transcript."
          : "Please add a clinician note.";
      return;
    }
    if (selectedFormats.size === 0) {
      error = "Please select at least one note type.";
      return;
    }
    if ($sidecarBusy) {
      error = "Another operation is in progress. Please wait or cancel it first.";
      return;
    }

    const ok = await ensureSidecar();
    if (!ok) {
      error = "Failed to start the processing engine.";
      return;
    }

    const sortedFormats = [...selectedFormats].sort((a, b) => a.localeCompare(b));
    try {
      await setPatientFormats(patientId, sortedFormats);
    } catch (e) {
      error = `Failed to save note type preferences: ${String(e)}`;
      return;
    }

    error = "";
    currentOperation.set(opId);
    progressBase.set(0);
    progressScale.set(100);
    progressPercent.set(0);
    progressStage.set(startsFromText ? "Saving source material..." : "Transcribing session recording...");
    activeOperation.set({
      type: startsFromText ? "create_session" : "transcribe",
      label: startsFromText ? "Saving source material..." : "Transcribing session recording...",
    });
    phase = startsFromText ? "generating" : "transcribing";

    let session: Session;
    try {
      session = await invoke<Session>("create_session", {
        data: {
          patient_id: patientId,
          date: new Date().toISOString().slice(0, 10),
        },
      });
      currentOperation.set(`${opId}-${session.id}`);
      onProcessingStart(session);
    } catch (e) {
      error = `Failed to create session: ${e}`;
      finishOperation();
      return;
    }

    let sourceText = "";
    let duration: number | null = null;
    let language: string | null = null;

    if (startsFromAudioFile) {
      try {
        const result = await transcribe(
          audioPath,
          sourceKind === "session_transcript" && diarizeSession,
        );
        sourceText = result.transcript;
        duration = result.duration;
        language = result.language;
      } catch (e) {
        const msg = String(e);
        error =
          msg === "sidecar_busy"
            ? "Another operation is in progress. Please wait or cancel it first."
            : `Transcription failed: ${msg}`;
        finishOperation();
        return;
      }
    } else {
      sourceText = cleanedText;
    }

    try {
      const input = await createSessionInput({
        session_id: session.id,
        kind: sourceKind,
        source: startsFromText ? SESSION_INPUT_SOURCES.typed : SESSION_INPUT_SOURCES.uploadAudio,
        title: sourceLabel,
        text: sourceText,
        audio_file: startsFromAudioFile ? audioPath : null,
        language: startsFromAudioFile ? language || null : null,
        duration_seconds: startsFromAudioFile ? duration : null,
        include_in_notes: true,
      });
      session = {
        ...session,
        inputs: [input],
      };
    } catch (e) {
      error = `Failed to save session: ${e}`;
      finishOperation();
      return;
    }

    phase = "generating";
    try {
      session = await generateSessionDocumentation(session, sortedFormats, {
        defaultLlm,
        thinking,
        verb: "Creating",
        onSessionUpdate: onComplete,
      });
    } catch (e) {
      const msg = String(e);
      error =
        msg === "sidecar_busy"
          ? "Another operation is in progress. Please wait or cancel it first."
          : msg;
      finishOperation();
      return;
    }

    progressPercent.set(100);
    finishOperation();
    onComplete(session);
  }
</script>

<div class="new-session-panel">
  <div class="new-session-heading">
    <h3>Create a new session</h3>
    <p>Choose how to add the first source material.</p>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div class="session-start-grid" role="radiogroup" aria-label="Choose how to add the first source material">
    <button
      type="button"
      class="session-start-card"
      class:active={selectedStartOption === "record_session"}
      onclick={() => selectStartOption("record_session")}
      disabled={phase !== "idle" || $isRecording}
      aria-checked={selectedStartOption === "record_session"}
      role="radio"
    >
      <span class="session-start-title">Record session</span>
      <span class="session-start-desc">Capture session audio and create a transcript.</span>
    </button>
    <button
      type="button"
      class="session-start-card"
      class:active={selectedStartOption === "upload_recording"}
      onclick={() => selectStartOption("upload_recording")}
      disabled={phase !== "idle" || $isRecording}
      aria-checked={selectedStartOption === "upload_recording"}
      role="radio"
    >
      <span class="session-start-title">Upload session recording</span>
      <span class="session-start-desc">Transcribe an existing audio file.</span>
    </button>
    <button
      type="button"
      class="session-start-card"
      class:active={selectedStartOption === "paste_transcript"}
      onclick={() => selectStartOption("paste_transcript")}
      disabled={phase !== "idle" || $isRecording}
      aria-checked={selectedStartOption === "paste_transcript"}
      role="radio"
    >
      <span class="session-start-title">Paste session transcript</span>
      <span class="session-start-desc">Use text from a completed session.</span>
    </button>
    <button
      type="button"
      class="session-start-card"
      class:active={selectedStartOption === "type_note"}
      onclick={() => selectStartOption("type_note")}
      disabled={phase !== "idle" || $isRecording}
      aria-checked={selectedStartOption === "type_note"}
      role="radio"
    >
      <span class="session-start-title">Write clinician note</span>
      <span class="session-start-desc">Add observations, corrections, and plan details.</span>
    </button>
    <button
      type="button"
      class="session-start-card"
      class:active={selectedStartOption === "dictate_note"}
      onclick={() => selectStartOption("dictate_note")}
      disabled={phase !== "idle" || $isRecording}
      aria-checked={selectedStartOption === "dictate_note"}
      role="radio"
    >
      <span class="session-start-title">Record clinician note</span>
      <span class="session-start-desc">Record your own post-session note.</span>
    </button>
  </div>

  {#if inputMethod === "audio_file"}
    <div class="new-session-row">
      <div class="form-group" style="flex: 1;">
        <label for="audio">Session recording</label>
        <div class="file-picker-row">
          <input
            bind:value={audioPath}
            placeholder="Select a session recording..."
            readonly
            disabled={phase !== "idle"}
          />
          <button class="btn" onclick={pickFile} disabled={phase !== "idle"}>Browse</button>
        </div>
        <label class="option-checkbox">
          <input type="checkbox" bind:checked={diarizeSession} disabled={phase !== "idle"} />
          <span>Identify speakers</span>
        </label>
      </div>
    </div>
  {:else if inputMethod === "recording" || inputMethod === "dictation"}
    <div class="record-section">
      {#if $isRecording}
        <div class="record-active">
          <div class="record-indicator">
            <span class="recording-dot-inline" class:paused={$recordingPaused}></span>
            {#if $recordingPaused}
              <span class="recording-paused-label">Paused</span>
            {/if}
            <span class="record-timer">{formatTimer($recordingElapsed)}</span>
          </div>
          <div class="record-level-meter">
            <div class="record-level-meter-fill" style="width: {Math.min($recordingLevel * 100, 100)}%;"></div>
          </div>
          <p class="record-hint">Recording in progress. You can navigate away and return while Gist keeps recording.</p>
          <div class="record-active-actions">
            <button class="btn record-pause-btn" onclick={handleToggleRecordingPause}>
              {$recordingPaused ? "Resume" : "Pause"}
            </button>
            <button class="btn btn-primary record-stop-btn" onclick={handleStopRecording}>
              Stop {recordingLabel}
            </button>
          </div>
        </div>
      {:else}
        <div class="record-controls">
          <div class="form-group">
            <label for="input-device">Microphone</label>
            <select id="input-device" bind:value={selectedInputDevice} disabled={phase !== "idle"}>
              {#each inputDevices as d (d.name)}
                <option value={d.name}>{d.name}</option>
              {/each}
            </select>
          </div>
          {#if inputMethod === "recording"}
            <div class="form-group">
            <label for="output-device">Computer audio</label>
              <select id="output-device" bind:value={selectedOutputDevice} disabled={phase !== "idle"}>
                {#each outputDevices as d (d.name)}
                  <option value={d.name}>{d.name}</option>
                {/each}
              </select>
            </div>
            <p class="record-hint">Computer audio capture requires macOS 14.4+. If it is unavailable, Gist will use the microphone.</p>
          {/if}
          {#if sourceKind === "session_transcript"}
            <label class="option-checkbox">
              <input type="checkbox" bind:checked={diarizeSession} disabled={phase !== "idle"} />
              <span>Identify speakers</span>
            </label>
          {/if}
          <button class="btn btn-primary record-start-btn" onclick={handleStartRecording} disabled={phase !== "idle"}>
            Start recording
          </button>
        </div>
      {/if}
    </div>
  {:else if inputMethod === "text"}
    <div class="form-group">
      <label for="session-text">{textLabel}</label>
      <textarea
        id="session-text"
        class="clinical-editor source-note-editor"
        bind:value={textDraft}
        placeholder={textPlaceholder}
        disabled={phase !== "idle"}
      ></textarea>
    </div>
  {/if}

  <div class="format-checklist" role="group" aria-labelledby="note-formats-label">
    <div id="note-formats-label" class="format-checklist-label">Notes to generate</div>
    <div class="format-checklist-items">
      {#if !formatsLoaded}
        <span class="text-muted">Loading note types...</span>
      {:else if visibleFormats.length === 0}
        <span class="text-muted">No note types available.</span>
      {:else}
        {#each visibleFormats as f (f.id)}
          <label class="format-checkbox" class:checked={selectedFormats.has(f.name)}>
            <input
              type="checkbox"
              checked={selectedFormats.has(f.name)}
              onchange={() => toggleFormat(f.name)}
              disabled={phase !== "idle" || $isRecording}
            />
            <span class="format-checkbox-label">{f.name.toUpperCase()}</span>
          </label>
        {/each}
      {/if}
    </div>
  </div>

  {#if !$isRecording}
    <div class="new-session-actions">
      {#if canSubmitDirectly}
        <button
          class="btn btn-primary"
          onclick={start}
          disabled={phase !== "idle" || !formatsLoaded || selectedFormats.size === 0 || (inputMethod === "text" ? !textDraft.trim() : !audioPath)}
        >
          {primaryActionLabel}
        </button>
      {/if}
      <button class="btn" onclick={onCancel} disabled={phase !== "idle"}>Cancel</button>
    </div>
  {/if}
</div>

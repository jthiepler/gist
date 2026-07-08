<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { marked } from "marked";
  import {
    createSessionInput,
    createSessionNote,
    getSession,
    getPatientFormats,
    listAudioDevices,
    pauseRecording,
    resumeRecording,
    startRecording,
    stopRecording,
    transcribe,
    updateSessionInput,
    type AudioDevice,
  } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import type { RecordingContext } from "$lib/processSession";
  import {
    getInputLabel,
    getSessionDurationSeconds,
    getSessionInput,
    getSessionLanguage,
    hasNoteSourceMaterial,
    SESSION_INPUT_KINDS,
    SESSION_INPUT_LABELS,
    SESSION_INPUT_SOURCES,
  } from "$lib/sessionInputs";
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
  import type { Session, SessionInput, SessionInputKind } from "$lib/types";

  let {
    session,
    isGenerating = false,
    onGenerateNote,
    onDelete,
    onChange,
  }: {
    session: Session;
    isGenerating?: boolean;
    onGenerateNote: (
      session: Session,
      options?: { regenerateExisting?: boolean }
    ) => void | Promise<void>;
    onDelete: (session: Session) => void;
    onChange: (session: Session) => void;
  } = $props();

  type EditTarget = "note" | "input";
  type ExistingInputMethod = "audio_file" | "recording" | "text" | "dictation";

  let expanded = $state(true);
  let showInputs = $state(false);
  let noteEditing = $state(false);
  let editingInputKind = $state<SessionInputKind | null>(null);
  let addingInputKind = $state<SessionInputKind | null>(null);
  let inputMethod = $state<ExistingInputMethod>("text");
  let noteDraft = $state("");
  let inputDraft = $state("");
  let inputAudioPath = $state("");
  let noteEditorEl = $state<HTMLTextAreaElement | null>(null);
  let inputEditorEl = $state<HTMLTextAreaElement | null>(null);
  let savingNote = $state(false);
  let savingInput = $state(false);
  let processingInput = $state(false);
  let noteStatus = $state("");
  let inputStatus = $state("");
  let defaultLlm = $state("");
  let defaultTranscription = $state("");
  let thinking = $state(false);
  let audioDevices = $state<AudioDevice[]>([]);
  let inputDevices = $state<AudioDevice[]>([]);
  let outputDevices = $state<AudioDevice[]>([]);
  let selectedInputDevice = $state("");
  let selectedOutputDevice = $state("");

  const addInputOperationId = $derived(`add-input-${session.id}`);

  let sortedNotes = $derived(
    [...session.notes].sort((a, b) => a.format.localeCompare(b.format))
  );

  let tabs = $derived(
    sortedNotes.map((n) => ({ key: n.format, label: n.format.toUpperCase(), note: n }))
  );

  let activeTab = $derived.by(() => {
    if (sortedNotes.length > 0) return sortedNotes[0].format;
    return "";
  });

  let currentTab = $state<string | null>(null);
  let activeKey = $derived(currentTab ?? activeTab);
  let activeNote = $derived(sortedNotes.find((n) => n.format === activeKey));
  let hasSources = $derived(hasNoteSourceMaterial(session));

  let formattedDate = $derived(
    new Date(session.date).toLocaleDateString("en-US", {
      year: "numeric",
      month: "long",
      day: "numeric",
    })
  );

  let durationMin = $derived(
    getSessionDurationSeconds(session)
      ? Math.round((getSessionDurationSeconds(session) ?? 0) / 60)
      : null
  );

  let language = $derived(getSessionLanguage(session));
  let noteDirty = $derived(activeNote ? noteDraft !== (activeNote.note ?? "") : false);
  let inputDirty = $derived(
    editingInputKind
      ? inputDraft !== (getSessionInput(session, editingInputKind)?.text ?? "")
      : false
  );
  let renderedNote = $derived(activeNote ? renderMarkdown(noteDraft) : "");
  let renderedInput = $derived(renderMarkdown(inputDraft));
  let recordingForThisSession = $derived($recordingContext?.session?.id === session.id);
  let recordingInputKind = $derived(
    recordingForThisSession ? $recordingContext?.inputKind ?? null : null
  );

  $effect(() => {
    const _ = session.id;
    currentTab = null;
    noteEditing = false;
    editingInputKind = null;
    addingInputKind = null;
    noteStatus = "";
    inputStatus = "";
    inputAudioPath = "";
    showInputs = sortedNotes.length === 0;
  });

  $effect(() => {
    const _ = `${session.id}:${activeNote?.id ?? ""}:${activeNote?.note ?? ""}`;
    noteEditing = false;
    noteDraft = activeNote?.note ?? "";
  });

  function renderMarkdown(content: string) {
    return marked.parse(content, { breaks: true }) as string;
  }

  function inputFor(kind: SessionInputKind) {
    return getSessionInput(session, kind);
  }

  function plainTextFromHtml(html: string) {
    const container = document.createElement("div");
    container.innerHTML = html;
    return container.innerText;
  }

  function statusIsError(status: string) {
    return (
      status.startsWith("Save failed") ||
      status.startsWith("Copy failed") ||
      status.startsWith("Transcription failed") ||
      status.startsWith("Failed") ||
      status.startsWith("Audio devices unavailable") ||
      status.startsWith("Another operation")
    );
  }

  function clearCopiedStatus(target: "note" | "input") {
    window.setTimeout(() => {
      if (target === "note" && noteStatus === "Copied") noteStatus = "";
      if (target === "input" && inputStatus === "Copied") inputStatus = "";
    }, 2000);
  }

  async function copyRenderedContent(target: "note" | "input", input?: SessionInput | null) {
    const html = target === "note" ? renderedNote : renderMarkdown(input?.text ?? "");
    const text = target === "note" ? (activeNote?.note ?? "") : (input?.text ?? "");

    try {
      if (navigator.clipboard.write && typeof ClipboardItem !== "undefined") {
        await navigator.clipboard.write([
          new ClipboardItem({
            "text/html": new Blob([html], { type: "text/html" }),
            "text/plain": new Blob([plainTextFromHtml(html) || text], {
              type: "text/plain",
            }),
          }),
        ]);
      } else {
        await navigator.clipboard.writeText(text);
      }
      if (target === "note") {
        noteStatus = "Copied";
      } else {
        inputStatus = "Copied";
      }
      clearCopiedStatus(target);
    } catch (e) {
      if (target === "note") {
        noteStatus = `Copy failed: ${String(e)}`;
      } else {
        inputStatus = `Copy failed: ${String(e)}`;
      }
    }
  }

  function toggle() {
    expanded = !expanded;
  }

  function selectTab(key: string) {
    currentTab = key;
    noteEditing = false;
    noteStatus = "";
  }

  function startNoteEditing() {
    if (!activeNote) return;
    noteDraft = activeNote.note ?? "";
    noteEditing = true;
    noteStatus = "";
  }

  function cancelNoteEditing() {
    noteDraft = activeNote?.note ?? "";
    noteEditing = false;
    noteStatus = "";
  }

  function startInputEditing(kind: SessionInputKind) {
    inputDraft = inputFor(kind)?.text ?? "";
    editingInputKind = kind;
    addingInputKind = null;
    inputMethod = "text";
    inputStatus = "";
    showInputs = true;
    queueMicrotask(() => inputEditorEl?.focus());
  }

  function cancelInputEditing() {
    inputDraft = "";
    editingInputKind = null;
    addingInputKind = null;
    inputAudioPath = "";
    inputStatus = "";
  }

  async function startInputAdding(kind: SessionInputKind, method: ExistingInputMethod) {
    if (processingInput || $isRecording) return;
    inputDraft = "";
    inputAudioPath = "";
    editingInputKind = method === "text" ? kind : null;
    addingInputKind = kind;
    inputMethod = method;
    inputStatus = "";
    showInputs = true;
    if (method === "text") {
      queueMicrotask(() => inputEditorEl?.focus());
    }
    if (method === "recording" || method === "dictation") {
      await loadAudioDevices();
    }
  }

  async function loadExistingInputSettings() {
    const settings = await loadSettings();
    defaultLlm = settings.defaultLlm;
    defaultTranscription = settings.defaultTranscription;
    thinking = settings.thinking;
  }

  async function loadAudioDevices() {
    await loadExistingInputSettings();
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
      inputStatus = `Audio devices unavailable: ${String(e)}`;
    }
  }

  async function pickInputAudioFile() {
    try {
      const path = await invoke<string | null>("pick_audio_file");
      if (path) inputAudioPath = path;
    } catch (e) {
      inputStatus = String(e);
    }
  }

  function handleEditorShortcut(event: KeyboardEvent, save: () => void) {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      save();
    }
  }

  function updateEditorDraft(target: EditTarget, value: string) {
    if (target === "note") {
      noteDraft = value;
    } else {
      inputDraft = value;
    }
  }

  function getEditorState(target: EditTarget) {
    if (target === "note") {
      return { element: noteEditorEl, value: noteDraft };
    }
    return { element: inputEditorEl, value: inputDraft };
  }

  function replaceSelection(
    target: EditTarget,
    nextValue: string,
    selectionStart: number,
    selectionEnd: number
  ) {
    updateEditorDraft(target, nextValue);
    queueMicrotask(() => {
      const { element } = getEditorState(target);
      if (!element) return;
      element.focus();
      element.setSelectionRange(selectionStart, selectionEnd);
    });
  }

  function normalizeSelection(value: string, start: number, end: number) {
    let normalizedEnd = end;
    while (normalizedEnd > start && /[\r\n]/.test(value[normalizedEnd - 1])) {
      normalizedEnd -= 1;
    }
    return { start, end: normalizedEnd };
  }

  function applyInlineMarkdown(
    target: EditTarget,
    prefix: string,
    suffix: string,
    placeholder: string
  ) {
    const { element, value } = getEditorState(target);
    if (!element) return;

    const selection = normalizeSelection(
      value,
      element.selectionStart ?? 0,
      element.selectionEnd ?? 0
    );
    const { start, end } = selection;
    const selected = value.slice(start, end);
    const content = selected || placeholder;
    const replacement = `${prefix}${content}${suffix}`;
    const nextValue = `${value.slice(0, start)}${replacement}${value.slice(end)}`;
    const selectionOffset = start + prefix.length;
    replaceSelection(target, nextValue, selectionOffset, selectionOffset + content.length);
  }

  function applyHeadingMarkdown(target: EditTarget, level: 1 | 2 | 3) {
    const { element, value } = getEditorState(target);
    if (!element) return;

    const selection = normalizeSelection(
      value,
      element.selectionStart ?? 0,
      element.selectionEnd ?? 0
    );
    const { start, end } = selection;
    const lineStart = value.lastIndexOf("\n", Math.max(0, start - 1)) + 1;
    const lineEndIndex = value.indexOf("\n", end);
    const lineEnd = lineEndIndex === -1 ? value.length : lineEndIndex;
    const selectedBlock = value.slice(lineStart, lineEnd);
    const prefix = `${"#".repeat(level)} `;
    const replacement = selectedBlock
      .split("\n")
      .map((line) => (line.trim().length === 0 ? line : `${prefix}${line.replace(/^#{1,6}\s+/, "")}`))
      .join("\n");
    const nextValue = `${value.slice(0, lineStart)}${replacement}${value.slice(lineEnd)}`;
    replaceSelection(target, nextValue, lineStart, lineStart + replacement.length);
  }

  async function saveNote() {
    if (!activeNote || savingNote || !noteDirty) return;
    savingNote = true;
    noteStatus = "";
    try {
      const saved = await createSessionNote(
        session.id,
        activeNote.format,
        noteDraft,
        activeNote.llm_model
      );
      onChange({
        ...session,
        notes: session.notes.map((n) => (n.format === saved.format ? saved : n)),
      });
      noteEditing = false;
      noteStatus = "Saved";
    } catch (e) {
      noteStatus = `Save failed: ${String(e)}`;
    } finally {
      savingNote = false;
    }
  }

  async function saveInput() {
    if (!editingInputKind || savingInput || !inputDraft.trim()) return;
    savingInput = true;
    inputStatus = "";
    try {
      const existing = inputFor(editingInputKind);
      const saved = existing
        ? await updateSessionInput({ id: existing.id, text: inputDraft })
        : await createSessionInput({
            session_id: session.id,
            kind: editingInputKind,
            source: SESSION_INPUT_SOURCES.typed,
            title: SESSION_INPUT_LABELS[editingInputKind],
            text: inputDraft,
            include_in_notes: true,
          });
      const inputs = existing
        ? session.inputs.map((input) => (input.id === saved.id ? saved : input))
        : [...session.inputs, saved];
      const updatedSession = { ...session, inputs };
      onChange(updatedSession);
      editingInputKind = null;
      addingInputKind = null;
      inputDraft = "";
      inputStatus = "Updating documentation...";
      await onGenerateNote(updatedSession, { regenerateExisting: true });
      inputStatus = "Saved";
    } catch (e) {
      inputStatus = `Save failed: ${String(e)}`;
    } finally {
      savingInput = false;
    }
  }

  async function saveAudioInput(kind: SessionInputKind) {
    if (processingInput || !inputAudioPath) return;
    if ($sidecarBusy) {
      inputStatus = "Another operation is in progress. Please wait or cancel it first.";
      return;
    }

    const ok = await ensureSidecar();
    if (!ok) {
      inputStatus = "Failed to start the processing engine.";
      return;
    }

    processingInput = true;
    inputStatus = "";
    currentOperation.set(addInputOperationId);
    progressBase.set(0);
    progressScale.set(100);
    progressPercent.set(0);
    progressStage.set("Transcribing session recording...");
    activeOperation.set({ type: "transcribe", label: "Transcribing session recording..." });

    try {
      await loadExistingInputSettings();
      const result = await transcribe(inputAudioPath, defaultTranscription || undefined);
      progressStage.set("Saving session material...");
      activeOperation.set({ type: "create_session", label: "Saving session material..." });
      await createSessionInput({
        session_id: session.id,
        kind,
        source: SESSION_INPUT_SOURCES.uploadAudio,
        title: SESSION_INPUT_LABELS[kind],
        text: result.transcript,
        audio_file: inputAudioPath,
        duration_seconds: result.duration,
        language: result.language || null,
        transcription_model: defaultTranscription || null,
        include_in_notes: true,
      });
      const updated = await getSession(session.id);
      if (updated) {
        onChange(updated);
        await onGenerateNote(updated, { regenerateExisting: true });
      }
      addingInputKind = null;
      inputAudioPath = "";
      inputStatus = "Saved";
    } catch (e) {
      const msg = String(e);
      inputStatus =
        msg === "sidecar_busy"
          ? "Another operation is in progress. Please wait or cancel it first."
          : `Transcription failed: ${msg}`;
    } finally {
      processingInput = false;
      progressStage.set("");
      activeOperation.set({ type: null, label: "" });
      currentOperation.set(null);
      progressBase.set(0);
      progressScale.set(100);
    }
  }

  function formatTimer(seconds: number): string {
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  async function startExistingRecording(kind: SessionInputKind, method: "recording" | "dictation") {
    if ($isRecording || processingInput) return;
    await loadAudioDevices();
    if (inputStatus.startsWith("Audio devices unavailable")) return;

    try {
      const ctx: RecordingContext = {
        patientId: session.patient_id,
        formats: await getPatientFormats(session.patient_id),
        defaultLlm,
        defaultTranscription,
        thinking,
        inputKind: kind,
        session,
      };
      recordingContext.set(ctx);
      await startRecording(
        selectedInputDevice || undefined,
        method === "recording" ? selectedOutputDevice || undefined : undefined
      );
      isRecording.set(true);
      recordingPaused.set(false);
      recordingElapsed.set(0);
      addingInputKind = kind;
      inputMethod = method;
      inputStatus = "";
    } catch (e) {
      inputStatus = `Failed to start ${method === "dictation" ? "dictation" : "recording"}: ${String(e)}`;
      recordingContext.set(null);
    }
  }

  async function handleStopRecording() {
    try {
      await stopRecording();
    } catch (e) {
      inputStatus = `Failed to stop recording: ${String(e)}`;
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
      inputStatus = `Failed to ${$recordingPaused ? "resume" : "pause"} recording: ${String(e)}`;
    }
  }
</script>

<div class="session-card" class:generating={isGenerating} class:collapsed={!expanded}>
  <div
    class="session-card-header"
    onclick={toggle}
    role="button"
    tabindex="0"
    onkeydown={(e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        toggle();
      }
    }}
  >
    <div class="session-header-left">
      <svg class="session-chevron" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="9 18 15 12 9 6"/>
      </svg>
      <div class="session-date">{formattedDate}</div>
    </div>
    <div class="session-meta">
      {#if durationMin}
        <span>{durationMin} min</span>
      {/if}
      {#if language}
        <span>{language}</span>
      {/if}
      <button
        class="btn btn-sm btn-danger delete-session-btn"
        onclick={(e) => { e.stopPropagation(); onDelete(session); }}
        disabled={isGenerating}
        title="Delete session"
        aria-label="Delete session"
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"/>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
        </svg>
      </button>
    </div>
  </div>

  {#if expanded}
    <div class="session-card-tabs-row">
      <div class="session-card-tabs">
        {#each tabs as tab (tab.key)}
          <button
            class="session-tab"
            class:active={activeKey === tab.key}
            onclick={() => selectTab(tab.key)}
            disabled={isGenerating}
          >
            {tab.label}
          </button>
        {/each}
      </div>
      <button
        class="btn btn-sm inputs-toggle"
        class:active={showInputs}
        onclick={() => showInputs = !showInputs}
        disabled={isGenerating}
        title={showInputs ? "Hide transcript and notes" : "Show transcript and notes"}
      >
        Transcript & Notes
      </button>
    </div>

    <div class="session-card-body">
      {#if isGenerating}
        <div class="spinner-container">
          <div class="spinner"></div>
          <p class="text-muted spinner-message">Generating note...</p>
        </div>
      {:else}
        <div class="session-editor-layout" class:with-inputs={showInputs && !!activeNote} class:inputs-only={!activeNote}>
          {#if activeNote}
            <section class="editor-pane">
              <div class="editor-pane-header">
                <div class="editor-pane-title">{activeNote.format.toUpperCase()} note</div>
                <div class="editor-actions">
                  {#if noteStatus}
                    <span class:error={statusIsError(noteStatus)} class="save-status">{noteStatus}</span>
                  {/if}
                  {#if noteEditing}
                    <button class="btn btn-sm" onclick={cancelNoteEditing} disabled={savingNote}>Cancel</button>
                    <button class="btn btn-sm btn-primary" onclick={saveNote} disabled={savingNote || !noteDirty}>
                      {savingNote ? "Saving..." : "Save"}
                    </button>
                  {:else}
                    <button class="btn btn-sm" onclick={() => copyRenderedContent("note")}>Copy</button>
                    <button class="btn btn-sm" onclick={startNoteEditing}>Edit</button>
                  {/if}
                </div>
              </div>

              {#if noteEditing}
                <div class="markdown-toolbar" role="toolbar" aria-label="Note formatting">
                  <button class="toolbar-btn" type="button" title="Heading 1" aria-label="Heading 1" onmousedown={(e) => e.preventDefault()} onclick={() => applyHeadingMarkdown("note", 1)}>H1</button>
                  <button class="toolbar-btn" type="button" title="Heading 2" aria-label="Heading 2" onmousedown={(e) => e.preventDefault()} onclick={() => applyHeadingMarkdown("note", 2)}>H2</button>
                  <button class="toolbar-btn" type="button" title="Heading 3" aria-label="Heading 3" onmousedown={(e) => e.preventDefault()} onclick={() => applyHeadingMarkdown("note", 3)}>H3</button>
                  <button class="toolbar-btn toolbar-btn-strong" type="button" title="Bold" aria-label="Bold" onmousedown={(e) => e.preventDefault()} onclick={() => applyInlineMarkdown("note", "**", "**", "bold text")}>B</button>
                  <button class="toolbar-btn toolbar-btn-emphasis" type="button" title="Italic" aria-label="Italic" onmousedown={(e) => e.preventDefault()} onclick={() => applyInlineMarkdown("note", "*", "*", "italic text")}>I</button>
                </div>
                <textarea
                  class="clinical-editor note-editor"
                  bind:this={noteEditorEl}
                  bind:value={noteDraft}
                  disabled={savingNote}
                  onkeydown={(e) => handleEditorShortcut(e, saveNote)}
                ></textarea>
              {:else}
                <div class="rendered-content markdown-content">{@html renderedNote}</div>
              {/if}
            </section>
          {/if}

          {#if showInputs || !activeNote}
            <section class="editor-pane inputs-pane">
              <div class="editor-pane-header">
                <div class="editor-pane-title">Transcript & Notes</div>
                <div class="editor-actions">
                  {#if inputStatus}
                    <span class:error={statusIsError(inputStatus)} class="save-status">{inputStatus}</span>
                  {/if}
                  {#if hasSources && !activeNote}
                    <button class="btn btn-sm btn-primary" onclick={() => onGenerateNote(session)}>
                      Generate Documentation
                    </button>
                  {/if}
                </div>
              </div>

              <div class="source-input-list">
                {#each SESSION_INPUT_KINDS as kind}
                  {@const input = inputFor(kind)}
                  <div class="source-input-card" class:empty={!input}>
                    <div class="source-input-header">
                      <div>
                        <div class="source-input-title">{SESSION_INPUT_LABELS[kind]}</div>
                        {#if input}
                          <div class="source-input-meta">{input.source.replace("_", " ")}</div>
                        {/if}
                      </div>
                      <div class="editor-actions">
                        {#if editingInputKind === kind}
                          <button class="btn btn-sm" onclick={cancelInputEditing} disabled={savingInput}>Cancel</button>
                          <button class="btn btn-sm btn-primary" onclick={saveInput} disabled={savingInput || !inputDirty || !inputDraft.trim()}>
                            {savingInput ? "Saving..." : "Save"}
                          </button>
                        {:else if addingInputKind === kind && inputMethod === "audio_file"}
                          <button class="btn btn-sm" onclick={cancelInputEditing} disabled={processingInput}>Cancel</button>
                          <button class="btn btn-sm btn-primary" onclick={() => saveAudioInput(kind)} disabled={processingInput || !inputAudioPath}>
                            {processingInput ? "Transcribing..." : "Transcribe & Save"}
                          </button>
                        {:else if addingInputKind === kind && (inputMethod === "recording" || inputMethod === "dictation") && !$isRecording}
                          <button class="btn btn-sm" onclick={cancelInputEditing}>Cancel</button>
                        {:else if input}
                          <button class="btn btn-sm" onclick={() => copyRenderedContent("input", input)}>
                            Copy
                          </button>
                          <button class="btn btn-sm" onclick={() => startInputEditing(kind)}>Edit</button>
                        {/if}
                      </div>
                    </div>

                    {#if editingInputKind === kind}
                      <div class="markdown-toolbar" role="toolbar" aria-label={`${SESSION_INPUT_LABELS[kind]} formatting`}>
                        <button class="toolbar-btn" type="button" title="Heading 1" aria-label="Heading 1" onmousedown={(e) => e.preventDefault()} onclick={() => applyHeadingMarkdown("input", 1)}>H1</button>
                        <button class="toolbar-btn" type="button" title="Heading 2" aria-label="Heading 2" onmousedown={(e) => e.preventDefault()} onclick={() => applyHeadingMarkdown("input", 2)}>H2</button>
                        <button class="toolbar-btn" type="button" title="Heading 3" aria-label="Heading 3" onmousedown={(e) => e.preventDefault()} onclick={() => applyHeadingMarkdown("input", 3)}>H3</button>
                        <button class="toolbar-btn toolbar-btn-strong" type="button" title="Bold" aria-label="Bold" onmousedown={(e) => e.preventDefault()} onclick={() => applyInlineMarkdown("input", "**", "**", "bold text")}>B</button>
                        <button class="toolbar-btn toolbar-btn-emphasis" type="button" title="Italic" aria-label="Italic" onmousedown={(e) => e.preventDefault()} onclick={() => applyInlineMarkdown("input", "*", "*", "italic text")}>I</button>
                      </div>
                      <textarea
                        class="clinical-editor source-input-editor"
                        bind:this={inputEditorEl}
                        bind:value={inputDraft}
                        disabled={savingInput}
                        onkeydown={(e) => handleEditorShortcut(e, saveInput)}
                      ></textarea>
                    {:else if addingInputKind === kind && inputMethod === "audio_file"}
                      <div class="source-input-add-panel">
                        <div class="file-picker-row">
                          <input
                            bind:value={inputAudioPath}
                            placeholder="Select a session recording..."
                            readonly
                            disabled={processingInput}
                          />
                          <button class="btn" onclick={pickInputAudioFile} disabled={processingInput}>Browse</button>
                        </div>
                      </div>
                    {:else if addingInputKind === kind && (inputMethod === "recording" || inputMethod === "dictation")}
                      <div class="source-input-add-panel">
                        {#if $isRecording && recordingForThisSession && recordingInputKind === kind}
                          <div class="record-active compact-record-active">
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
                            <div class="record-active-actions">
                              <button class="btn record-pause-btn" onclick={handleToggleRecordingPause}>
                                {$recordingPaused ? "Resume" : "Pause"}
                              </button>
                              <button class="btn btn-primary record-stop-btn" onclick={handleStopRecording}>
                                Stop {inputMethod === "dictation" ? "Dictation" : "Recording"}
                              </button>
                            </div>
                          </div>
                        {:else}
                          <div class="record-controls compact-record-controls">
                            <div class="form-group">
                              <label for={`input-device-${session.id}-${kind}`}>Microphone</label>
                              <select id={`input-device-${session.id}-${kind}`} bind:value={selectedInputDevice}>
                                {#each inputDevices as d (d.name)}
                                  <option value={d.name}>{d.name}</option>
                                {/each}
                              </select>
                            </div>
                            {#if inputMethod === "recording"}
                              <div class="form-group">
                                <label for={`output-device-${session.id}-${kind}`}>System audio</label>
                                <select id={`output-device-${session.id}-${kind}`} bind:value={selectedOutputDevice}>
                                  {#each outputDevices as d (d.name)}
                                    <option value={d.name}>{d.name}</option>
                                  {/each}
                                </select>
                              </div>
                            {/if}
                            <button class="btn btn-primary record-start-btn" onclick={() => startExistingRecording(kind, inputMethod === "dictation" ? "dictation" : "recording")}>
                              {inputMethod === "dictation" ? "Start Dictation" : "Start Recording"}
                            </button>
                          </div>
                        {/if}
                      </div>
                    {:else if input}
                      <div class="rendered-content markdown-content source-input-rendered">
                        {@html renderMarkdown(input.text)}
                      </div>
                    {:else}
                      <div class="source-input-empty">
                        <p>
                          {kind === "session_transcript"
                            ? "Add session material when it becomes available."
                            : "Add your observations, corrections, formulation, or plan details."}
                        </p>
                        <div class="source-add-methods">
                          {#if kind === "session_transcript"}
                            <button class="session-start-card source-add-method" onclick={() => startInputAdding(kind, "recording")} disabled={$isRecording || processingInput}>
                              <span class="session-start-title">Record session</span>
                              <span class="session-start-desc">Capture session audio and create a transcript.</span>
                            </button>
                            <button class="session-start-card source-add-method" onclick={() => startInputAdding(kind, "audio_file")} disabled={$isRecording || processingInput}>
                              <span class="session-start-title">Upload session recording</span>
                              <span class="session-start-desc">Transcribe an existing audio file.</span>
                            </button>
                            <button class="session-start-card source-add-method" onclick={() => startInputAdding(kind, "text")} disabled={$isRecording || processingInput}>
                              <span class="session-start-title">Paste session transcript</span>
                              <span class="session-start-desc">Use text from a completed session.</span>
                            </button>
                          {:else}
                            <button class="session-start-card source-add-method" onclick={() => startInputAdding(kind, "text")} disabled={$isRecording || processingInput}>
                              <span class="session-start-title">Type clinician note</span>
                              <span class="session-start-desc">Add observations, corrections, and plan details.</span>
                            </button>
                            <button class="session-start-card source-add-method" onclick={() => startInputAdding(kind, "dictation")} disabled={$isRecording || processingInput}>
                              <span class="session-start-title">Dictate clinician note</span>
                              <span class="session-start-desc">Record your own post-session note.</span>
                            </button>
                          {/if}
                        </div>
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>

              {#if !hasSources}
                <div class="spinner-container note-empty-panel">
                  <p class="text-muted empty-message">Add a session transcript or clinician note to generate documentation.</p>
                </div>
              {/if}
            </section>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

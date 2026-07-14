<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ask, confirm } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { renderSafeMarkdown } from "$lib/markdown";
  import { formatLocalDate } from "$lib/date";
  import MarkdownToolbar from "$lib/components/MarkdownToolbar.svelte";
  import {
    createSessionInput,
    createSessionNote,
    deleteSessionInput,
    getSession,
    listAudioDevices,
    listNoteFormats,
    pauseRecording,
    resumeRecording,
    startRecording,
    stopRecording,
    transcribe,
    updateSession,
    updateSessionInput,
    type AudioDevice,
  } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import {
    DEFAULT_DIARIZATION_SPEAKERS,
    DIARIZATION_SPEAKER_COUNTS,
  } from "$lib/diarization";
  import type { RecordingContext } from "$lib/processSession";
  import {
    getSessionDurationSeconds,
    getInputLabel,
    SESSION_INPUT_LABELS,
    SESSION_INPUT_SOURCES,
  } from "$lib/sessionInputs";
  import { confirmRegenerateAttachedNotes } from "$lib/confirmations";
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
  import type { NoteFormatTemplate, Session, SessionInput, SessionInputKind } from "$lib/types";

  let {
    session,
    initiallyExpanded = true,
    isGenerating = false,
    deletionDisabled = false,
    onGenerateNote,
    onDelete,
    onChange,
  }: {
    session: Session;
    initiallyExpanded?: boolean;
    isGenerating?: boolean;
    deletionDisabled?: boolean;
    onGenerateNote: (
      session: Session,
      options?: { regenerateExisting?: boolean; formats?: string[] }
    ) => boolean | Promise<boolean>;
    onDelete: (session: Session) => void;
    onChange: (session: Session) => void;
  } = $props();

  type EditTarget = "note" | "input";
  type ExistingInputMethod = "audio_file" | "recording" | "text" | "dictation";
  type WorkspaceMode = "focus" | "compare";
  type FocusPane = "note" | "source" | "details";

  let expanded = $state(false);
  let workspaceMode = $state<WorkspaceMode>("focus");
  let focusPane = $state<FocusPane>("note");
  let singleViewPane = $state<FocusPane>("note");
  let noteEditing = $state(false);
  let editingInputId = $state<string | null>(null);
  let addingInputKind = $state<SessionInputKind | null>(null);
  let inputMethod = $state<ExistingInputMethod>("text");
  let noteDraft = $state("");
  let inputDraft = $state("");
  let inputAudioPath = $state("");
  let diarizeInput = $state(true);
  let diarizationSpeakers = $state<number>(DEFAULT_DIARIZATION_SPEAKERS);
  let noteEditorEl = $state<HTMLTextAreaElement | null>(null);
  let inputEditorEl = $state<HTMLTextAreaElement | null>(null);
  let savingNote = $state(false);
  let savingInput = $state(false);
  let processingInput = $state(false);
  let noteStatus = $state("");
  let inputStatus = $state("");
  let metadataStatus = $state("");
  let metadataEditing = $state(false);
  let metadataDate = $state("");
  let metadataTime = $state("");
  let metadataTitle = $state("");
  let transcriptSearch = $state("");
  let defaultLlm = $state("");
  let thinking = $state(false);
  let confirmRecordingConsent = $state(true);
  let recordingConsentConfirmed = $state(false);
  let pendingInputRefreshSession = $state<Session | null>(null);
  let openSessionMenu = $state(false);
  let openNoteMenu = $state(false);
  let openAddNoteMenu = $state(false);
  let openInputMenu = $state<string | null>(null);
  let openAddSourceMenu = $state(false);
  let expandedInputId = $state<string | null>(null);
  let initializedSessionId = $state<string | null>(null);
  let previousNoteCount = $state(0);
  let inlineRecordingStarted = $state(false);
  let noteFormats = $state<NoteFormatTemplate[]>([]);
  let noteFormatsLoading = $state(false);
  let generatingFormat = $state<string | null>(null);
  let noteSaveTimer: ReturnType<typeof setTimeout> | null = null;
  let inputSaveTimer: ReturnType<typeof setTimeout> | null = null;

  let inputDevices = $state<AudioDevice[]>([]);
  let outputDevices = $state<AudioDevice[]>([]);
  let selectedInputDevice = $state("");
  let selectedOutputDevice = $state("");
  let sessionMenuTrigger = $state<HTMLButtonElement | null>(null);

  const addInputOperationId = $derived(`add-input-${session.id}`);

  let sortedNotes = $derived(
    [...session.notes].sort((a, b) => a.format.localeCompare(b.format))
  );

  let tabs = $derived(
    sortedNotes.map((note) => ({ key: note.format, label: note.format.toUpperCase() }))
  );

  let activeTab = $derived(sortedNotes[0]?.format ?? "");

  let currentTab = $state<string | null>(null);
  let activeKey = $derived(currentTab ?? activeTab);
  let activeNote = $derived(sortedNotes.find((n) => n.format === activeKey));
  let sourceInputs = $derived(
    session.inputs.filter((input) => input.include_in_notes && input.text.trim().length > 0)
  );
  let visibleSourceInputs = $derived.by(() => {
    const query = transcriptSearch.trim().toLocaleLowerCase();
    if (!query) return session.inputs;
    return session.inputs.filter((input) => {
      const searchableText = [
        getInputLabel(input),
        input.kind,
        input.source,
        input.title,
        input.text,
      ]
        .filter(Boolean)
        .join(" ")
        .toLocaleLowerCase();
      return searchableText.includes(query);
    });
  });
  let hasSources = $derived(sourceInputs.length > 0);
  let missingNoteFormats = $derived(
    noteFormats
      .filter((format) => !format.hidden && !session.notes.some((note) => note.format === format.name))
      .sort((a, b) => a.name.localeCompare(b.name))
  );
  let sessionSummary = $derived.by(() => {
    const parts: string[] = [];
    if (session.inputs.length > 0) {
      parts.push(`${session.inputs.length} ${session.inputs.length === 1 ? "source" : "sources"}`);
    }
    if (sortedNotes.length > 0) {
      parts.push(`${sortedNotes.length} ${sortedNotes.length === 1 ? "note" : "notes"}`);
    }
    return parts.join(" · ");
  });

  let formattedDate = $derived(
    formatLocalDate(session.date, {
      year: "numeric",
      month: "long",
      day: "numeric",
    }, "en-US")
  );

  let formattedTime = $derived.by(() => {
    const time = session.start_time || (session.date.includes("T") ? session.date.split("T")[1] : "");
    if (!time) return "";
    const [hours, minutes] = time.split(":").map(Number);
    const value = new Date(2000, 0, 1, hours, minutes);
    return value.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  });

  let lastEdited = $derived.by(() => {
    const timestamps = [
      session.updated_at,
      session.created_at,
      ...session.inputs.map((input) => input.updated_at),
      ...session.notes.map((note) => note.created_at),
    ].filter(Boolean) as string[];
    const latest = timestamps.sort((a, b) => Date.parse(b) - Date.parse(a))[0];
    return latest ? new Date(latest).toLocaleDateString([], { month: "short", day: "numeric" }) : "";
  });

  let durationSeconds = $derived(getSessionDurationSeconds(session));
  let durationMin = $derived(
    durationSeconds ? Math.round(durationSeconds / 60) : null
  );

  let noteDirty = $derived(activeNote ? noteDraft !== (activeNote.note ?? "") : false);
  let inputDirty = $derived(
    editingInputId
      ? inputDraft !== (session.inputs.find((input) => input.id === editingInputId)?.text ?? "")
      : addingInputKind !== null && inputMethod === "text" && inputDraft.trim().length > 0
  );
  let notesNeedRefresh = $derived.by(() => {
    if (session.notes.length === 0) return false;
    const latestSourceUpdate = Math.max(
      ...session.inputs
        .filter((input) => input.include_in_notes)
        .map((input) => Date.parse(input.updated_at)),
      0,
    );
    const oldestNoteUpdate = Math.min(
      ...session.notes.map((note) => Date.parse(note.created_at)),
    );
    return latestSourceUpdate > oldestNoteUpdate;
  });
  let renderedNote = $derived(activeNote ? renderMarkdown(noteDraft) : "");
  let recordingForThisSession = $derived($recordingContext?.session?.id === session.id);
  let recordingInputKind = $derived(
    recordingForThisSession ? $recordingContext?.inputKind ?? null : null
  );
  let processingThisSession = $derived(
    isGenerating ||
      $currentOperation === addInputOperationId ||
      $currentOperation === `gen-note-${session.id}` ||
    $currentOperation === `new-session-${session.id}`
  );
  let processingLabel = $derived(
    $activeOperation.label || (isGenerating ? "Generating notes..." : "Processing...")
  );

  $effect(() => {
    if (
      !inlineRecordingStarted ||
      $isRecording ||
      recordingForThisSession
    ) {
      return;
    }

    inlineRecordingStarted = false;
    addingInputKind = null;
    inputDraft = "";
    inputAudioPath = "";
    diarizeInput = true;
    diarizationSpeakers = DEFAULT_DIARIZATION_SPEAKERS;
  });

  $effect(() => {
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (openSessionMenu) {
        event.preventDefault();
        openSessionMenu = false;
        queueMicrotask(() => sessionMenuTrigger?.focus());
      } else if (openNoteMenu || openInputMenu || openAddSourceMenu) {
        event.preventDefault();
        openNoteMenu = false;
        openInputMenu = null;
        openAddSourceMenu = false;
      }
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  });

  $effect(() => {
    const dismissMenus = (event: PointerEvent) => {
      if ((event.target as Element).closest(".action-menu")) return;
      openSessionMenu = false;
      openNoteMenu = false;
      openInputMenu = null;
      openAddSourceMenu = false;
    };

    document.addEventListener("pointerdown", dismissMenus);
    return () => document.removeEventListener("pointerdown", dismissMenus);
  });

  $effect(() => {
    const sessionId = session.id;
    if (initializedSessionId === sessionId) return;

    initializedSessionId = sessionId;
    expanded = initiallyExpanded;
    currentTab = null;
    noteEditing = false;
    editingInputId = null;
    addingInputKind = null;
    noteStatus = "";
    inputStatus = "";
    inputAudioPath = "";
    openSessionMenu = false;
    openNoteMenu = false;
    openInputMenu = null;
    openAddSourceMenu = false;
    expandedInputId = null;
    workspaceMode = "focus";
    focusPane = untrack(() => (sortedNotes.length === 0 ? "source" : "note"));
    singleViewPane = focusPane;
    previousNoteCount = untrack(() => sortedNotes.length);
    metadataEditing = false;
    metadataDate = session.date.slice(0, 10);
    metadataTime = session.start_time ?? (session.date.includes("T") ? session.date.split("T")[1]?.slice(0, 5) ?? "" : "");
    metadataTitle = session.title ?? "";
  });

  // Reveal the first generated note as soon as it is saved. Subsequent notes
  // add tabs without pulling the user away from the note they are reviewing.
  $effect(() => {
    const noteCount = sortedNotes.length;
    if (
      initializedSessionId === session.id &&
      previousNoteCount === 0 &&
      noteCount > 0 &&
      focusPane === "source"
    ) {
      currentTab = sortedNotes[0].format;
      focusPane = "note";
      expanded = true;
    }
    previousNoteCount = noteCount;
  });

  $effect(() => {
    const _ = `${session.id}:${activeNote?.id ?? ""}:${activeNote?.note ?? ""}`;
    if (!noteEditing) noteDraft = activeNote?.note ?? "";
  });

  onDestroy(() => {
    if (noteSaveTimer) clearTimeout(noteSaveTimer);
    if (inputSaveTimer) clearTimeout(inputSaveTimer);
    if (noteDirty) void saveNote(true);
    if (inputDirty && editingInputId) void saveInput(true);
  });

  function renderMarkdown(content: string) {
    return renderSafeMarkdown(content);
  }

  async function openMarkdownLink(event: MouseEvent) {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const link = target.closest("a");
    if (!(link instanceof HTMLAnchorElement) || !link.href) return;
    event.preventDefault();
    try {
      await openUrl(link.href);
    } catch (e) {
      const status = `Could not open link: ${String(e)}`;
      if ((event.currentTarget as Element).classList.contains("source-input-rendered")) {
        inputStatus = status;
      } else {
        noteStatus = status;
      }
    }
  }

  function openMarkdownLinkFromKeyboard(event: KeyboardEvent) {
    if (event.key !== "Enter" && event.key !== " ") return;
    if (!(event.target instanceof HTMLAnchorElement)) return;
    event.preventDefault();
    void openMarkdownLink(event as unknown as MouseEvent);
  }

  function escapeHtml(value: string) {
    return value.replace(/[&<>"']/g, (character) => ({
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    })[character] ?? character);
  }

  function escapeRegExp(value: string) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }

  function highlightSearchText(text: string) {
    const query = transcriptSearch.trim();
    if (!query) return escapeHtml(text);

    const pattern = new RegExp(escapeRegExp(query), "gi");
    let html = "";
    let lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = pattern.exec(text)) !== null) {
      html += escapeHtml(text.slice(lastIndex, match.index));
      html += `<mark>${escapeHtml(match[0])}</mark>`;
      lastIndex = match.index + match[0].length;
    }
    return html + escapeHtml(text.slice(lastIndex));
  }

  function highlightRenderedMarkdown(content: string) {
    const html = renderMarkdown(content);
    const query = transcriptSearch.trim();
    if (!query) return html;

    const pattern = new RegExp(escapeRegExp(query), "gi");
    return html.replace(/(<[^>]*>)|([^<]+)/g, (segment, tag) =>
      tag ? segment : segment.replace(pattern, "<mark>$&</mark>")
    );
  }

  function inputFor(id: string) {
    return session.inputs.find((input) => input.id === id) ?? null;
  }

  function sourcePreview(input: SessionInput) {
    const text = input.text.replace(/\s+/g, " ").trim();
    if (!text) return "No text yet";

    const query = transcriptSearch.trim().toLocaleLowerCase();
    const matchIndex = query ? text.toLocaleLowerCase().indexOf(query) : -1;
    if (matchIndex >= 0) {
      const contextStart = Math.max(0, matchIndex - 60);
      const contextEnd = Math.min(text.length, contextStart + 160);
      return `${contextStart > 0 ? "..." : ""}${text.slice(contextStart, contextEnd)}${contextEnd < text.length ? "..." : ""}`;
    }

    return text.length > 160 ? `${text.slice(0, 157)}...` : text;
  }

  function sourceMetadata(input: SessionInput) {
    const parts: string[] = [];
    if (input.duration_seconds) parts.push(`${Math.round(input.duration_seconds / 60)} min`);
    parts.push(input.source === "typed" ? "Clinician authored" : input.audio_file ? "Transcribed locally" : "Text source");
    parts.push(`Added ${new Date(input.created_at).toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" })}`);
    return parts.join(" · ");
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
      status.startsWith("Delete failed") ||
      status.startsWith("Transcription failed") ||
      status.startsWith("Regeneration failed") ||
      status.startsWith("Failed") ||
      status.startsWith("Audio devices unavailable") ||
      status.startsWith("Another operation")
    );
  }

  async function confirmDocumentationRefresh(sessionToRefresh = session) {
    if (sessionToRefresh.notes.length === 0) return false;
    return confirmRegenerateAttachedNotes(sessionToRefresh.notes.length);
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
        openNoteMenu = false;
      } else {
        inputStatus = "Copied";
        openInputMenu = null;
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

  async function toggle() {
    if (noteDirty && !(await saveNote(true))) return;
    if (inputDirty && editingInputId && !(await saveInput(true))) return;
    expanded = !expanded;
    openSessionMenu = false;
    openNoteMenu = false;
    openInputMenu = null;
    openAddSourceMenu = false;
  }

  function setWorkspaceMode(mode: WorkspaceMode) {
    if (mode === workspaceMode) return;
    if (mode === "compare") {
      singleViewPane = focusPane;
    }
    workspaceMode = mode;
    if (mode === "compare" && (focusPane === "note" || focusPane === "details")) {
      focusPane = "source";
    }
    if (mode === "focus") {
      focusPane = singleViewPane === "note" && activeNote ? "note" : singleViewPane;
    }
  }

  function setFocusPane(pane: FocusPane) {
    focusPane = pane;
    if (workspaceMode === "focus") singleViewPane = pane;
  }

  function startMetadataEditing() {
    metadataDate = session.date.slice(0, 10);
    metadataTime = session.start_time ?? (session.date.includes("T") ? session.date.split("T")[1]?.slice(0, 5) ?? "" : "");
    metadataTitle = session.title ?? "";
    metadataEditing = true;
    metadataStatus = "";
  }

  async function saveMetadata() {
    if (!metadataDate) return;
    metadataStatus = "Saving…";
    try {
      const updated = await updateSession({
        id: session.id,
        date: metadataDate,
        start_time: metadataTime || null,
        title: metadataTitle.trim() || null,
        session_type: session.session_type,
      });
      onChange(updated);
      metadataEditing = false;
      metadataStatus = "Saved";
    } catch (e) {
      metadataStatus = `Could not save: ${String(e)}`;
    }
  }

  async function selectTab(key: string) {
    if (noteDirty && !(await saveNote(true))) return;
    currentTab = key;
    if (workspaceMode === "focus") focusPane = "note";
    noteEditing = false;
    noteStatus = "";
    openNoteMenu = false;
  }

  function startNoteEditing() {
    if (!activeNote) return;
    noteDraft = activeNote.note ?? "";
    noteEditing = true;
    noteStatus = "";
    openNoteMenu = false;
  }

  async function loadAvailableNoteFormats() {
    if (noteFormatsLoading) return;
    noteFormatsLoading = true;
    try {
      noteFormats = await listNoteFormats();
    } catch (e) {
      noteStatus = `Could not load note types: ${String(e)}`;
    } finally {
      noteFormatsLoading = false;
    }
  }

  async function toggleAddNoteMenu() {
    openAddNoteMenu = !openAddNoteMenu;
    if (openAddNoteMenu) await loadAvailableNoteFormats();
  }

  async function addNote(format: string) {
    if (!hasSources || isGenerating) return;
    openAddNoteMenu = false;
    generatingFormat = format;
    noteStatus = `Generating ${format.toUpperCase()} note…`;
    const generated = await onGenerateNote(session, { formats: [format] });
    if (generated) {
      currentTab = format;
      focusPane = "note";
      noteStatus = "Generated";
    }
    generatingFormat = null;
  }

  async function regenerateActiveNote() {
    if (!activeNote || !hasSources || isGenerating) return;
    openNoteMenu = false;
    const format = activeNote.format;
    const approved = await confirm(
      `Regenerate the ${format.toUpperCase()} note? This will replace the current note, including any manual edits.`,
      { title: "Regenerate note", kind: "warning" }
    );
    if (!approved) return;

    if (noteSaveTimer) {
      clearTimeout(noteSaveTimer);
      noteSaveTimer = null;
    }
    noteEditing = false;
    noteDraft = activeNote.note ?? "";
    generatingFormat = format;
    noteStatus = "Regenerating…";
    const generated = await onGenerateNote(session, {
      regenerateExisting: true,
      formats: [format],
    });
    noteStatus = generated ? "Regenerated" : "Regeneration failed";
    generatingFormat = null;
  }

  async function closeNoteEditing() {
    if (noteSaveTimer) clearTimeout(noteSaveTimer);
    if (noteDirty && !(await saveNote(false))) return;
    noteEditing = false;
  }

  function startInputEditing(input: SessionInput) {
    inputDraft = input.text;
    editingInputId = input.id;
    addingInputKind = null;
    inputMethod = "text";
    inputStatus = "";
    pendingInputRefreshSession = null;
    openInputMenu = null;
    expandedInputId = input.id;
    focusPane = "source";
    queueMicrotask(() => inputEditorEl?.focus());
  }

  async function closeInputEditing() {
    if (inputSaveTimer) clearTimeout(inputSaveTimer);
    if (editingInputId && inputDirty && !(await saveInput(false))) return;
    if (
      editingInputId &&
      pendingInputRefreshSession &&
      !(await offerDocumentationRefresh(pendingInputRefreshSession))
    ) {
      // The source remains saved and the persistent out-of-date indicator
      // stays visible when the user chooses not to replace attached notes.
    }
    inputDraft = "";
    editingInputId = null;
    addingInputKind = null;
    inputAudioPath = "";
    inputStatus = "";
  }

  async function startInputAdding(kind: SessionInputKind, method: ExistingInputMethod) {
    if (processingInput || $isRecording) return;
    inputDraft = "";
    inputAudioPath = "";
    editingInputId = null;
    addingInputKind = kind;
    inputMethod = method;
    inputStatus = "";
    openInputMenu = null;
    openAddSourceMenu = false;
    expandedInputId = null;
    focusPane = "source";
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
    confirmRecordingConsent = settings.confirmRecordingConsent;
  }

  async function loadAudioDevices() {
    await loadExistingInputSettings();
    try {
      const devices = await listAudioDevices();
      inputDevices = devices.filter((d) => d.device_type === "input");
      outputDevices = devices.filter((d) => d.device_type === "output");
      if (inputDevices.length > 0 && !selectedInputDevice) {
        selectedInputDevice = inputDevices[0].id;
      }
      if (outputDevices.length > 0 && !selectedOutputDevice) {
        selectedOutputDevice = outputDevices[0].id;
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

  function queueAutosave(target: EditTarget) {
    if (target === "input" && !editingInputId) return;
    const existingTimer = target === "note" ? noteSaveTimer : inputSaveTimer;
    if (existingTimer) clearTimeout(existingTimer);
    const timer = setTimeout(() => {
      if (target === "note") {
        noteSaveTimer = null;
        void saveNote(true);
      } else {
        inputSaveTimer = null;
        void saveInput(true);
      }
    }, 800);
    if (target === "note") noteSaveTimer = timer;
    else inputSaveTimer = timer;
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
    queueAutosave(target);
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

  async function saveNote(autosave = false): Promise<boolean> {
    if (!activeNote || !noteDirty) return true;
    if (savingNote) return false;
    savingNote = true;
    noteStatus = "";
    const draftAtStart = noteDraft;
    try {
      const saved = await createSessionNote(
        session.id,
        activeNote.format,
        draftAtStart,
        activeNote.llm_model
      );
      onChange({
        ...session,
        notes: session.notes.map((n) => (n.format === saved.format ? saved : n)),
      });
      if (!autosave) noteEditing = false;
      noteStatus = autosave ? "Saved automatically" : "Saved";
      return true;
    } catch (e) {
      noteStatus = `Save failed: ${String(e)}`;
      return false;
    } finally {
      savingNote = false;
      if (noteEditing && noteDraft !== draftAtStart) queueAutosave("note");
    }
  }

  async function saveInput(autosave = false): Promise<boolean> {
    if (!editingInputId && !addingInputKind) return true;
    if (!editingInputId && !inputDraft.trim()) return true;
    if (editingInputId && !inputDirty) {
      if (!autosave && pendingInputRefreshSession) {
        await offerDocumentationRefresh(pendingInputRefreshSession);
      }
      return true;
    }
    if (savingInput) return false;
    savingInput = true;
    inputStatus = "";
    const draftAtStart = inputDraft;
    try {
      const existing = editingInputId ? inputFor(editingInputId) : null;
      const saved = existing
        ? await updateSessionInput({ id: existing.id, text: draftAtStart })
        : await createSessionInput({
            session_id: session.id,
            kind: addingInputKind!,
            source: SESSION_INPUT_SOURCES.typed,
            title: SESSION_INPUT_LABELS[addingInputKind!],
            text: draftAtStart,
            include_in_notes: true,
          });
      const inputs = existing
        ? session.inputs.map((input) => (input.id === saved.id ? saved : input))
        : [...session.inputs, saved];
      const updatedSession = { ...session, inputs };
      onChange(updatedSession);
      if (updatedSession.notes.length > 0) {
        pendingInputRefreshSession = updatedSession;
      }
      const refreshed = !autosave && pendingInputRefreshSession
        ? await offerDocumentationRefresh(updatedSession)
        : false;
      if (!autosave || !existing) {
        editingInputId = null;
        addingInputKind = null;
        inputDraft = "";
      }
      inputStatus = refreshed
        ? "Notes updated"
        : autosave && updatedSession.notes.length > 0
          ? "Saved automatically — attached notes need updating"
          : autosave
            ? "Saved automatically"
            : "Saved";
      return true;
    } catch (e) {
      inputStatus = `Save failed: ${String(e)}`;
      return false;
    } finally {
      savingInput = false;
      if (editingInputId && inputDraft !== draftAtStart) queueAutosave("input");
    }
  }

  async function offerDocumentationRefresh(sessionToRefresh: Session): Promise<boolean> {
    if (!(await confirmDocumentationRefresh(sessionToRefresh))) {
      pendingInputRefreshSession = null;
      return false;
    }
    inputStatus = "Updating notes...";
    const refreshed = await onGenerateNote(sessionToRefresh, { regenerateExisting: true });
    if (refreshed) pendingInputRefreshSession = null;
    return refreshed;
  }

  async function refreshOutdatedNotes() {
    await offerDocumentationRefresh(pendingInputRefreshSession ?? session);
  }

  async function deleteInput(input: SessionInput) {
    if (deletionDisabled || processingThisSession || processingInput) {
      inputStatus = "Finish the active processing task before deleting source material.";
      return;
    }
    const label = getInputLabel(input);
    if (
      !(await confirm(`Delete this ${label.toLowerCase()}? This source will be removed from future notes.`, {
        title: "Delete source material",
        kind: "warning",
      }))
    ) {
      return;
    }

    inputStatus = "";
    try {
      await deleteSessionInput(input.id);
      const updatedSession = {
        ...session,
        inputs: session.inputs.filter((existing) => existing.id !== input.id),
      };
      onChange(updatedSession);
      openInputMenu = null;
      expandedInputId = null;
      if (await confirmDocumentationRefresh(updatedSession)) {
        inputStatus = "Updating notes...";
        await onGenerateNote(updatedSession, { regenerateExisting: true });
      }
      inputStatus = "Source deleted";
    } catch (e) {
      inputStatus = `Delete failed: ${String(e)}`;
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
      const result = await transcribe(
        inputAudioPath,
        kind === "session_transcript" && diarizeInput,
        diarizationSpeakers,
        defaultLlm,
      );
      progressStage.set("Saving source material...");
      activeOperation.set({ type: "create_session", label: "Saving source material..." });
      await createSessionInput({
        session_id: session.id,
        kind,
        source: SESSION_INPUT_SOURCES.uploadAudio,
        title: SESSION_INPUT_LABELS[kind],
        text: result.transcript,
        audio_file: inputAudioPath,
        duration_seconds: result.duration,
        metadata_json: JSON.stringify({ segments: result.segments }),
        include_in_notes: true,
      });
      const updated = await getSession(session.id);
      if (updated) {
        onChange(updated);
        if (await confirmDocumentationRefresh(updated)) {
          await onGenerateNote(updated, { regenerateExisting: true });
        }
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
    if (inputDevices.length === 0) {
      inputStatus = "No microphone is available. Connect one and allow Gist to use it in macOS Privacy & Security.";
      return;
    }
    if (method === "recording" && !selectedOutputDevice) {
      inputStatus = "Select a computer-audio device before starting the session recording.";
      return;
    }
    if (confirmRecordingConsent && !recordingConsentConfirmed) {
      inputStatus = "Confirm recording consent before starting.";
      return;
    }

    try {
      const ctx: RecordingContext = {
        patientId: session.patient_id,
        formats: session.notes.map((note) => note.format),
        defaultLlm,
        thinking,
        inputKind: kind,
        diarize: kind === "session_transcript" && diarizeInput,
        numSpeakers: diarizationSpeakers,
        session,
      };
      const job = await startRecording({
        session_id: session.id,
        input_kind: kind,
        formats: ctx.formats,
        llm_model: defaultLlm,
        thinking,
        diarize: ctx.diarize,
        num_speakers: ctx.numSpeakers,
        created_session: false,
      },
        selectedInputDevice || undefined,
        method === "recording" ? selectedOutputDevice || undefined : undefined
      );
      recordingContext.set({ ...ctx, jobId: job.id });
      isRecording.set(true);
      recordingPaused.set(false);
      recordingElapsed.set(0);
      inlineRecordingStarted = true;
      addingInputKind = kind;
      inputMethod = method;
      inputStatus = "";
      recordingConsentConfirmed = false;
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
  <div class="session-card-header">
    <button class="session-disclosure" onclick={toggle} aria-expanded={expanded} aria-controls={`session-content-${session.id}`}>
      <div class="session-header-left">
      <svg class="session-chevron" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="9 18 15 12 9 6"/>
      </svg>
      <div class="session-date-block">
        <div class="session-title">{session.title || formattedDate}</div>
        {#if formattedTime || session.inputs.length === 0}
          <div class="session-row-subtitle">{formattedTime || "No source material yet"}</div>
        {/if}
      </div>
      {#if processingThisSession}
        <div class="session-processing-status" title={processingLabel}>
          <span class="session-processing-spinner" aria-hidden="true"></span>
          <span>{processingLabel}</span>
        </div>
      {/if}
      </div>
    </button>
    <div class="session-meta">
      {#if durationMin}
        <span>{durationMin} min</span>
      {/if}
      {#if sessionSummary}
        <span>{sessionSummary}</span>
      {/if}
      {#if lastEdited}
        <span>Edited {lastEdited}</span>
      {/if}
      <div class="action-menu">
        <button
          class="icon-btn action-menu-trigger"
          bind:this={sessionMenuTrigger}
          onclick={(e) => {
            e.stopPropagation();
            openSessionMenu = !openSessionMenu;
          }}
          disabled={isGenerating || deletionDisabled}
          title="Session actions"
          aria-label="Session actions"
          aria-expanded={openSessionMenu}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <circle cx="5" cy="12" r="1.8"/>
            <circle cx="12" cy="12" r="1.8"/>
            <circle cx="19" cy="12" r="1.8"/>
          </svg>
        </button>
        {#if openSessionMenu}
          <div class="action-menu-popover" role="menu" aria-label="Session actions">
            <button
              class="action-menu-item danger"
              role="menuitem"
              disabled={deletionDisabled}
              onclick={(e) => {
                e.stopPropagation();
                openSessionMenu = false;
                onDelete(session);
              }}
            >
              Delete session
            </button>
          </div>
        {/if}
      </div>
    </div>
  </div>

  {#if expanded}
    <div id={`session-content-${session.id}`} class="session-card-tabs-row">
      <div class="session-card-tabs">
        {#each tabs as tab (tab.key)}
          <button
            class="session-tab"
            class:active={activeKey === tab.key && (workspaceMode === "compare" || focusPane === "note")}
            onclick={() => selectTab(tab.key)}
            disabled={isGenerating}
          >
            Note · {tab.label}
          </button>
        {/each}
        <button
          class="session-tab"
          class:active={focusPane === "source"}
          onclick={() => setFocusPane("source")}
          disabled={isGenerating}
          aria-pressed={focusPane === "source"}
        >
          Session material
          <span class="session-tab-count" aria-hidden="true">{session.inputs.length}</span>
        </button>
        <button
          class="session-tab"
          class:active={workspaceMode === "focus" && focusPane === "details"}
          onclick={() => setFocusPane("details")}
          disabled={isGenerating || workspaceMode === "compare"}
          aria-pressed={workspaceMode === "focus" && focusPane === "details"}
        >
          Details
        </button>
      </div>
      <div class="action-menu add-note-menu">
          <button
            class="session-tab add-note-trigger"
            onclick={toggleAddNoteMenu}
            disabled={isGenerating || !hasSources || (noteFormats.length > 0 && missingNoteFormats.length === 0)}
            aria-expanded={openAddNoteMenu}
            title={!hasSources
              ? "Add source material before generating a note"
              : noteFormats.length > 0 && missingNoteFormats.length === 0
                ? "All note types have been added"
                : "Generate another note type"}
          >
            + Add note
          </button>
          {#if openAddNoteMenu}
            <div class="action-menu-popover add-note-popover" role="menu" aria-label="Add note">
              {#if noteFormatsLoading}
                <div class="action-menu-message">Loading note types…</div>
              {:else if missingNoteFormats.length === 0}
                <div class="action-menu-message">All note types added</div>
              {:else}
                {#each missingNoteFormats as format (format.id)}
                  <button
                    class="action-menu-item"
                    role="menuitem"
                    onclick={() => addNote(format.name)}
                  >
                    <strong>{format.name.toUpperCase()}</strong>
                    <span>{format.is_builtin ? format.name.toUpperCase() + " note" : format.name}</span>
                  </button>
                {/each}
              {/if}
            </div>
          {/if}
      </div>
      {#if activeNote}
        <div class="workspace-controls" aria-label="Workspace view">
          <div class="workspace-segmented" role="group" aria-label="Workspace mode">
            <button
              class:active={workspaceMode === "focus"}
              onclick={() => setWorkspaceMode("focus")}
              disabled={isGenerating}
              aria-pressed={workspaceMode === "focus"}
            >
              Single
            </button>
            <button
              class:active={workspaceMode === "compare"}
              onclick={() => setWorkspaceMode("compare")}
              disabled={isGenerating}
              aria-pressed={workspaceMode === "compare"}
            >
              Split
            </button>
          </div>
        </div>
      {/if}
    </div>

    <div class="session-card-body">
      {#if isGenerating && !activeNote}
        <div class="spinner-container">
          <div class="spinner"></div>
          <p class="text-muted spinner-message">Generating notes...</p>
        </div>
      {:else}
        <div
          class="session-editor-layout"
          class:focus-mode={workspaceMode === "focus"}
          class:compare-mode={workspaceMode === "compare" && !!activeNote}
          class:inputs-only={!activeNote}
        >
          {#if activeNote && (workspaceMode === "compare" || focusPane === "note")}
            <section class="editor-pane">
              <div
                class="editor-pane-header note-pane-header"
                class:has-draft={!noteEditing && !!activeNote?.llm_model}
              >
                {#if !noteEditing && activeNote?.llm_model}
                  <div class="note-draft-notice" role="note">
                    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <circle cx="12" cy="12" r="9"></circle>
                      <path d="M12 10v6"></path>
                      <path d="M12 7h.01"></path>
                    </svg>
                    <span>
                      <strong>AI-generated draft</strong>
                      Generated locally. Review before using in the clinical record.
                    </span>
                  </div>
                {/if}
                <div class="editor-actions">
                  {#if noteStatus}
                    <span class:error={statusIsError(noteStatus)} class="save-status" aria-live="polite">{noteStatus}</span>
                  {/if}
                  {#if !noteEditing}
                    <div class="action-menu">
                      <button
                        class="icon-btn action-menu-trigger"
                        onclick={() => openNoteMenu = !openNoteMenu}
                        title="Note actions"
                        aria-label="Note actions"
                        aria-expanded={openNoteMenu}
                      >
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                          <circle cx="5" cy="12" r="1.8"/>
                          <circle cx="12" cy="12" r="1.8"/>
                          <circle cx="19" cy="12" r="1.8"/>
                        </svg>
                      </button>
                      {#if openNoteMenu}
                        <div class="action-menu-popover" role="menu" aria-label="Note actions">
                          <button class="action-menu-item" role="menuitem" onclick={() => copyRenderedContent("note")}>
                            Copy note
                          </button>
                          <button class="action-menu-item" role="menuitem" onclick={startNoteEditing}>
                            Edit note
                          </button>
                          <button
                            class="action-menu-item"
                            role="menuitem"
                            onclick={regenerateActiveNote}
                            disabled={!hasSources || generatingFormat === activeNote.format}
                          >
                            Regenerate note
                          </button>
                        </div>
                      {/if}
                    </div>
                  {/if}
                </div>
              </div>

              {#if noteEditing}
                <MarkdownToolbar
                  target="note"
                  onHeading={(level) => applyHeadingMarkdown("note", level)}
                  onInline={(prefix, suffix, placeholder) => applyInlineMarkdown("note", prefix, suffix, placeholder)}
                />
                <textarea
                  class="clinical-editor note-editor"
                  bind:this={noteEditorEl}
                  bind:value={noteDraft}
                  disabled={savingNote}
                  oninput={() => queueAutosave("note")}
                  onkeydown={(e) => handleEditorShortcut(e, saveNote)}
                ></textarea>
                <div class="editor-footer">
                  <button class="btn btn-sm" onclick={closeNoteEditing} disabled={savingNote}>Close</button>
                  <button class="btn btn-sm btn-primary" onclick={() => saveNote()} disabled={savingNote || !noteDirty}>
                    {savingNote ? "Saving..." : "Save"}
                  </button>
                </div>
              {:else}
                <div
                  class="rendered-content markdown-content"
                  role="presentation"
                  onclick={openMarkdownLink}
                  onkeydown={openMarkdownLinkFromKeyboard}
                >{@html renderedNote}</div>
              {/if}
            </section>
          {/if}

          {#if focusPane === "details" && workspaceMode === "focus"}
            <section class="editor-pane details-pane">
              <div class="editor-pane-header">
                <div>
                  <div class="editor-pane-title">Session details</div>
                  <div class="editor-pane-subtitle">Appointment information associated with this session</div>
                </div>
                {#if metadataStatus}<span class:error={metadataStatus.startsWith("Could not")} class="save-status" aria-live="polite">{metadataStatus}</span>{/if}
              </div>

              <div class="session-details-form">
                <div class="form-group">
                  <label for={`metadata-date-${session.id}`}>Session date</label>
                  <input id={`metadata-date-${session.id}`} type="date" bind:value={metadataDate} disabled={!metadataEditing} />
                </div>
                <div class="form-group">
                  <label for={`metadata-time-${session.id}`}>Start time <span class="text-muted">(optional)</span></label>
                  <input id={`metadata-time-${session.id}`} type="time" bind:value={metadataTime} disabled={!metadataEditing} />
                </div>
                <div class="form-group">
                  <label for={`metadata-title-${session.id}`}>Session title <span class="text-muted">(optional)</span></label>
                  <input id={`metadata-title-${session.id}`} bind:value={metadataTitle} placeholder="Initial assessment" disabled={!metadataEditing} />
                </div>
              </div>
              <div class="details-summary">
                <span><strong>Duration</strong>{durationMin ? `${durationMin} min` : "Not available"}</span>
                <span><strong>Sources</strong>{session.inputs.length}</span>
                <span><strong>Notes</strong>{session.notes.length}</span>
              </div>
              <div class="editor-footer">
                {#if metadataEditing}
                  <button class="btn" onclick={() => { metadataEditing = false; metadataStatus = ""; }}>Cancel</button>
                  <button class="btn btn-primary" onclick={saveMetadata} disabled={!metadataDate}>Save details</button>
                {:else}
                  <button class="btn btn-primary" onclick={startMetadataEditing}>Edit details</button>
                {/if}
              </div>
            </section>
          {/if}

          {#if focusPane === "source" || (!activeNote && focusPane === "note")}
            <section class="editor-pane inputs-pane" class:supporting={workspaceMode === "compare" && !!activeNote}>
              <div class="editor-pane-header">
                <div class="editor-actions">
                  {#if inputStatus}
                    <span class:error={statusIsError(inputStatus)} class="save-status" aria-live="polite">{inputStatus}</span>
                  {/if}
                  {#if notesNeedRefresh}
                    <span class="save-status" aria-live="polite">Sources changed since these notes were generated.</span>
                    <button class="btn btn-sm" onclick={refreshOutdatedNotes} disabled={isGenerating || processingInput}>
                      Update notes
                    </button>
                  {/if}
                  {#if hasSources && !activeNote}
                    <button class="btn btn-sm btn-primary" onclick={() => onGenerateNote(session)}>
                      Generate notes
                    </button>
                  {/if}

                  {#if session.inputs.length > 0}
                    <div class="source-search">
                      <input
                        bind:value={transcriptSearch}
                        type="search"
                        placeholder="Search source materials"
                        aria-label="Search source materials"
                      />
                    </div>
                  {/if}

                  <div class="action-menu">
                    <button
                      class="btn btn-sm"
                      onclick={() => openAddSourceMenu = !openAddSourceMenu}
                      disabled={$isRecording || processingInput || !!editingInputId || !!addingInputKind}
                      aria-expanded={openAddSourceMenu}
                    >
                      Add source
                    </button>

                    {#if openAddSourceMenu}
                      <div class="action-menu-popover source-add-menu" role="menu" aria-label="Add source">
                        <button class="action-menu-item" role="menuitem" onclick={() => startInputAdding("session_transcript", "recording")}>
                          Record session
                        </button>
                        <button class="action-menu-item" role="menuitem" onclick={() => startInputAdding("session_transcript", "audio_file")}>
                          Upload session recording
                        </button>
                        <button class="action-menu-item" role="menuitem" onclick={() => startInputAdding("session_transcript", "text")}>
                          Paste session transcript
                        </button>
                        <button class="action-menu-item" role="menuitem" onclick={() => startInputAdding("clinician_note", "text")}>
                          Write clinician note
                        </button>
                        <button class="action-menu-item" role="menuitem" onclick={() => startInputAdding("clinician_note", "dictation")}>
                          Record clinician note
                        </button>
                      </div>
                    {/if}
                  </div>
                </div>
              </div>

              <div class="source-input-list">
                {#each visibleSourceInputs as input (input.id)}
                  <div class="source-input-card">
                    <div class="source-input-header">
                      <div>
                        <div class="source-input-title">
                          {@html highlightSearchText(getInputLabel(input))}
                        </div>
                        <div class="source-input-meta">{@html highlightSearchText(sourceMetadata(input))}</div>
                        <button
                          class="source-input-preview"
                          onclick={() => expandedInputId = expandedInputId === input.id ? null : input.id}
                          aria-expanded={expandedInputId === input.id}
                        >
                          {@html highlightSearchText(sourcePreview(input))}
                        </button>
                      </div>
                      <div class="editor-actions">
                        {#if editingInputId !== input.id}
                          <div class="action-menu">
                            <button
                              class="icon-btn action-menu-trigger"
                              onclick={() => openInputMenu = openInputMenu === input.id ? null : input.id}
                              title="Source actions"
                              aria-label="Source actions"
                              aria-expanded={openInputMenu === input.id}
                            >
                              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                                <circle cx="5" cy="12" r="1.8"/>
                                <circle cx="12" cy="12" r="1.8"/>
                                <circle cx="19" cy="12" r="1.8"/>
                              </svg>
                            </button>
                            {#if openInputMenu === input.id}
                              <div class="action-menu-popover" role="menu" aria-label="Source actions">
                                <button
                                  class="action-menu-item"
                                  role="menuitem"
                                  onclick={() => copyRenderedContent("input", input)}
                                >
                                  Copy source
                                </button>
                                <button
                                  class="action-menu-item"
                                  role="menuitem"
                                  onclick={() => startInputEditing(input)}
                                >
                                  Edit source
                                </button>
                                <button
                                  class="action-menu-item danger"
                                  role="menuitem"
                                  onclick={() => deleteInput(input)}
                                >
                                  Delete source
                                </button>
                              </div>
                            {/if}
                          </div>
                        {/if}
                      </div>
                    </div>

                    {#if editingInputId === input.id}
                      <MarkdownToolbar
                        target="input"
                        onHeading={(level) => applyHeadingMarkdown("input", level)}
                        onInline={(prefix, suffix, placeholder) => applyInlineMarkdown("input", prefix, suffix, placeholder)}
                      />
                      <textarea
                        class="clinical-editor source-input-editor"
                        bind:this={inputEditorEl}
                        bind:value={inputDraft}
                        disabled={savingInput}
                        oninput={() => queueAutosave("input")}
                        onkeydown={(e) => handleEditorShortcut(e, saveInput)}
                      ></textarea>
                      <div class="editor-footer">
                        <button class="btn btn-sm" onclick={closeInputEditing} disabled={savingInput}>Close</button>
                        <button class="btn btn-sm btn-primary" onclick={() => saveInput()} disabled={savingInput || !inputDirty}>
                          {savingInput ? "Saving..." : "Save"}
                        </button>
                      </div>
                    {:else if expandedInputId === input.id}
                      <div
                        class="rendered-content markdown-content source-input-rendered"
                        role="presentation"
                        onclick={openMarkdownLink}
                        onkeydown={openMarkdownLinkFromKeyboard}
                      >
                        {@html highlightRenderedMarkdown(input.text)}
                      </div>
                    {/if}
                  </div>
                {/each}

                {#if session.inputs.length > 0 && visibleSourceInputs.length === 0}
                  <p class="text-muted source-search-empty">No session material matches “{transcriptSearch}”.</p>
                {/if}

                {#if addingInputKind}
                  <div class="source-input-card empty">
                    <div class="source-input-header">
                      <div class="source-input-title">{SESSION_INPUT_LABELS[addingInputKind]}</div>
                      {#if inputMethod === "audio_file" || (inputMethod === "recording" || inputMethod === "dictation") && !$isRecording}
                        <button class="btn btn-sm" onclick={closeInputEditing} disabled={processingInput}>Cancel</button>
                      {/if}
                    </div>

                    {#if inputMethod === "text"}
                      <MarkdownToolbar
                        target="input"
                        onHeading={(level) => applyHeadingMarkdown("input", level)}
                        onInline={(prefix, suffix, placeholder) => applyInlineMarkdown("input", prefix, suffix, placeholder)}
                      />
                      <textarea
                        class="clinical-editor source-input-editor"
                        bind:this={inputEditorEl}
                        bind:value={inputDraft}
                        disabled={savingInput}
                        onkeydown={(e) => handleEditorShortcut(e, saveInput)}
                      ></textarea>
                      <div class="editor-footer">
                        <button class="btn btn-sm" onclick={closeInputEditing} disabled={savingInput}>Cancel</button>
                        <button class="btn btn-sm btn-primary" onclick={() => saveInput()} disabled={savingInput || !inputDirty}>
                          {savingInput ? "Saving..." : "Save"}
                        </button>
                      </div>
                    {:else if inputMethod === "audio_file"}
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
                        {#if addingInputKind === "session_transcript"}
                          <label class="option-checkbox">
                            <input type="checkbox" bind:checked={diarizeInput} disabled={processingInput} />
                            <span>Identify speakers</span>
                          </label>
                          {#if diarizeInput}
                            <label class="diarization-speaker-select" for={`input-file-speaker-count-${session.id}`}>
                              <span>Number of speakers</span>
                              <select id={`input-file-speaker-count-${session.id}`} bind:value={diarizationSpeakers} disabled={processingInput}>
                                {#each DIARIZATION_SPEAKER_COUNTS as speakerCount}
                                  <option value={speakerCount}>{speakerCount}</option>
                                {/each}
                              </select>
                            </label>
                          {/if}
                        {/if}
                        <div class="editor-footer">
                          <button class="btn btn-sm btn-primary" onclick={() => saveAudioInput(addingInputKind!)} disabled={processingInput || !inputAudioPath}>
                            {processingInput ? "Transcribing..." : "Transcribe and add"}
                          </button>
                        </div>
                      </div>
                    {:else}
                      <div class="source-input-add-panel">
                        {#if $isRecording && recordingForThisSession && recordingInputKind === addingInputKind}
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
                                Stop recording
                              </button>
                            </div>
                          </div>
                        {:else}
                          <div class="record-controls compact-record-controls">
                            <div class="form-group">
                              <label for={`input-device-${session.id}-${addingInputKind}`}>Microphone</label>
                              <select id={`input-device-${session.id}-${addingInputKind}`} bind:value={selectedInputDevice}>
                                {#each inputDevices as d (d.id)}
                                  <option value={d.id}>{d.name}</option>
                                {/each}
                              </select>
                            </div>
                            {#if inputMethod === "recording"}
                              <div class="form-group">
                              <label for={`output-device-${session.id}-${addingInputKind}`}>Computer audio</label>
                                <select id={`output-device-${session.id}-${addingInputKind}`} bind:value={selectedOutputDevice}>
                                  {#each outputDevices as d (d.id)}
                                    <option value={d.id}>{d.name}</option>
                                  {/each}
                                </select>
                              </div>
                            {/if}
                            {#if addingInputKind === "session_transcript"}
                              <label class="option-checkbox">
                                <input type="checkbox" bind:checked={diarizeInput} />
                                <span>Identify speakers</span>
                              </label>
                              {#if diarizeInput}
                                <label class="diarization-speaker-select" for={`input-recording-speaker-count-${session.id}`}>
                                  <span>Number of speakers</span>
                                  <select id={`input-recording-speaker-count-${session.id}`} bind:value={diarizationSpeakers}>
                                    {#each DIARIZATION_SPEAKER_COUNTS as speakerCount}
                                      <option value={speakerCount}>{speakerCount}</option>
                                    {/each}
                                  </select>
                                </label>
                              {/if}
                            {/if}
                            {#if confirmRecordingConsent}
                              <label class="recording-consent">
                                <input type="checkbox" bind:checked={recordingConsentConfirmed} disabled={processingInput} />
                                <span>I have confirmed consent to record according to my organization’s and jurisdiction’s requirements.</span>
                              </label>
                            {/if}
                            <p class="record-hint">About 345 MB per hour (roughly 690 MB for two hours). Gist prevents idle sleep while recording and processing.</p>
                            <button class="btn btn-primary record-start-btn" onclick={() => startExistingRecording(addingInputKind!, inputMethod === "dictation" ? "dictation" : "recording")} disabled={processingInput || inputDevices.length === 0 || (inputMethod === "recording" && !selectedOutputDevice) || (confirmRecordingConsent && !recordingConsentConfirmed)}>
                              Start recording
                            </button>
                          </div>
                        {/if}
                      </div>
                    {/if}
                  </div>
                {/if}
              </div>

              {#if !hasSources}
                <div class="spinner-container note-empty-panel">
                  <p class="text-muted empty-message">Add a session transcript or clinician note to generate notes.</p>
                </div>
              {/if}
            </section>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

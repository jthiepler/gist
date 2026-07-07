<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { transcribe, generateNote, listNoteFormats, createSessionNote, getPatientFormats, setPatientFormats } from "$lib/rpc";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import { progressPercent, progressStage, progressBase, progressScale, currentOperation, sidecarBusy, activeOperation } from "$lib/stores";
  import { loadSettings } from "$lib/settings";
  import type { Session, NoteFormatTemplate } from "$lib/types";

  let {
    patientId,
    onComplete,
    onCancel,
  }: {
    patientId: string;
    onComplete: (session: Session) => void;
    onCancel: () => void;
  } = $props();

  let audioPath = $state("");
  let formats = $state<NoteFormatTemplate[]>([]);
  let selectedFormats = $state<Set<string>>(new Set());
  let formatsLoaded = $state(false);
  let error = $state("");
  let phase = $state<"idle" | "transcribing" | "generating">("idle");
  let generatingLabel = $state("");

  // Settings loaded from SQLite
  let defaultLlm = $state("");
  let defaultTranscription = $state("");
  let thinking = $state(false);

  const opId = "new-session";

  let visibleFormats = $derived(formats.filter((f) => !f.hidden).sort((a, b) => a.name.localeCompare(b.name)));

  function toggleFormat(name: string) {
    const next = new Set(selectedFormats);
    if (next.has(name)) {
      next.delete(name);
    } else {
      next.add(name);
    }
    selectedFormats = next;
  }

  // Load formats + settings + patient's last format selection
  $effect(() => {
    (async () => {
      const ok = await ensureSidecar();
      if (!ok) return;
      try {
        formats = await listNoteFormats();
        // Load patient's last format selection
        const saved = await getPatientFormats(patientId);
        const visibleNames = formats.filter((f) => !f.hidden).map((f) => f.name);
        if (saved.length > 0) {
          const valid = saved.filter((n) => visibleNames.includes(n));
          selectedFormats = new Set(valid.length > 0 ? valid : [visibleFormats[0]?.name].filter(Boolean) as string[]);
        } else {
          // Default: select first visible format
          const first = visibleFormats[0];
          if (first) selectedFormats = new Set([first.name]);
        }
      } catch (e) {
        console.error("Failed to load formats/patient formats:", e);
      }
      formatsLoaded = true;

      const s = await loadSettings();
      if (s.defaultLlm) defaultLlm = s.defaultLlm;
      if (s.defaultTranscription) defaultTranscription = s.defaultTranscription;
      thinking = s.thinking;
    })();
  });

  // Cleanup progress on unmount — but DON'T cancel the operation
  $effect(() => {
    return () => {
      if ($currentOperation === opId) {
        currentOperation.set(null);
      }
    };
  });

  async function pickFile() {
    try {
      const path = await invoke<string | null>("pick_audio_file");
      if (path) audioPath = path;
    } catch (e) {
      error = String(e);
    }
  }

  async function start() {
    if (!audioPath) {
      error = "Please select an audio file.";
      return;
    }

    if (selectedFormats.size === 0) {
      error = "Please select at least one note format.";
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

    // Save format selection for this patient
    const sortedFormats = [...selectedFormats].sort((a, b) => a.localeCompare(b));
    await setPatientFormats(patientId, sortedFormats);

    error = "";
    currentOperation.set(opId);
    progressBase.set(0);
    progressScale.set(100);
    progressPercent.set(0);
    progressStage.set("Transcribing...");
    activeOperation.set({ type: "transcribe", label: "Transcribing audio..." });
    phase = "transcribing";

    let transcript = "";
    let duration: number | null = null;
    let language: string | null = null;

    try {
      const result = await transcribe(
        audioPath,
        defaultTranscription || undefined
      );
      transcript = result.transcript;
      duration = result.duration;
      language = result.language;
    } catch (e) {
      const msg = String(e);
      if (msg === "sidecar_busy") {
        error = "Another operation is in progress. Please wait or cancel it first.";
      } else {
        error = `Transcription failed: ${msg}`;
      }
      phase = "idle";
      progressStage.set("");
      activeOperation.set({ type: null, label: "" });
      currentOperation.set(null);
      progressBase.set(0);
      progressScale.set(100);
      return;
    }

    // Create session in DB
    let session: Session;
    try {
      session = await invoke<Session>("create_session", {
        data: {
          patient_id: patientId,
          date: new Date().toISOString().slice(0, 10),
          audio_file: audioPath,
        },
      });
      await invoke("update_session", {
        data: {
          id: session.id,
          transcript,
          language: language || null,
          duration_seconds: duration,
          transcription_model: defaultTranscription || null,
        },
      });
      session = {
        ...session,
        transcript,
        language: language || null,
        duration_seconds: duration,
        transcription_model: defaultTranscription || null,
      };
    } catch (e) {
      error = `Failed to save session: ${e}`;
      phase = "idle";
      progressStage.set("");
      activeOperation.set({ type: null, label: "" });
      currentOperation.set(null);
      return;
    }

    // Generate notes for each selected format (alphabetical)
    phase = "generating";

    // Load templates for prompts
    let templates: NoteFormatTemplate[] = [];
    try {
      templates = await listNoteFormats();
    } catch {}

    const totalNotes = sortedFormats.length;
    const basePct = 30;
    const noteRange = 70; // 30% → 100%

    for (let i = 0; i < sortedFormats.length; i++) {
      const fmtName = sortedFormats[i];
      generatingLabel = `Generating ${fmtName.toUpperCase()} note (${i + 1}/${totalNotes})...`;
      progressStage.set(generatingLabel);
      activeOperation.set({ type: "generate_note", label: generatingLabel });
      const fmtBase = basePct + Math.round((i / totalNotes) * noteRange);
      const fmtScale = Math.round(noteRange / totalNotes);
      progressBase.set(fmtBase);
      progressScale.set(fmtScale);

      try {
        const tmpl = templates.find((t) => t.name === fmtName);
        const result = await generateNote(
          transcript,
          fmtName,
          defaultLlm || undefined,
          thinking,
          tmpl?.prompt
        );
        const sn = await createSessionNote(session.id, fmtName, result.note, defaultLlm || null);
        session.notes = [...session.notes, sn];
      } catch (e) {
        const msg = String(e);
        if (msg === "sidecar_busy") {
          error = "Another operation is in progress. Please wait or cancel it first.";
        } else {
          error = `${fmtName.toUpperCase()} note generation failed: ${msg}`;
        }
        // Session was saved with transcript — call onComplete so user sees it
        onComplete(session);
        return;
      }
    }

    progressPercent.set(100);
    progressStage.set("");
    activeOperation.set({ type: null, label: "" });
    currentOperation.set(null);
    progressBase.set(0);
    progressScale.set(100);
    phase = "idle";
    onComplete(session);
  }
</script>

<div class="new-session-panel">
  <h3>New Session</h3>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div class="new-session-row">
    <div class="form-group" style="flex: 1;">
      <label for="audio">Audio File</label>
      <div class="file-picker-row">
        <input
          bind:value={audioPath}
          placeholder="Select an audio file..."
          readonly
          disabled={phase !== "idle"}
        />
        <button class="btn" onclick={pickFile} disabled={phase !== "idle"}>Browse</button>
      </div>
    </div>
  </div>

  <div class="format-checklist">
    <label class="format-checklist-label">Note Formats</label>
    <div class="format-checklist-items">
      {#if !formatsLoaded}
        <span class="text-muted">Loading...</span>
      {:else if visibleFormats.length === 0}
        <span class="text-muted">No formats available</span>
      {:else}
        {#each visibleFormats as f (f.id)}
          <label class="format-checkbox" class:checked={selectedFormats.has(f.name)}>
            <input
              type="checkbox"
              checked={selectedFormats.has(f.name)}
              onchange={() => toggleFormat(f.name)}
              disabled={phase !== "idle"}
            />
            <span class="format-checkbox-label">{f.name.toUpperCase()}</span>
          </label>
        {/each}
      {/if}
    </div>
  </div>

  <div class="new-session-actions">
    <button class="btn btn-primary" onclick={start} disabled={phase !== "idle" || !audioPath || !formatsLoaded || selectedFormats.size === 0}>
      {#if phase === "transcribing"}
        Transcribing...
      {:else if phase === "generating"}
        Generating Notes...
      {:else}
        Start
      {/if}
    </button>
    <button class="btn" onclick={onCancel} disabled={phase !== "idle"}>Cancel</button>
  </div>
</div>

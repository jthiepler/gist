<script lang="ts">
  import { onMount } from "svelte";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import {
    createNoteFormat,
    deleteNoteFormat,
    listNoteFormats,
    resetNoteFormat,
    toggleNoteFormatHidden,
    updateNoteFormat,
  } from "$lib/rpc";
  import type { NoteFormatTemplate } from "$lib/types";

  let formats = $state<NoteFormatTemplate[]>([]);
  let loading = $state(true);
  let error = $state("");
  let saved = $state(false);

  let editingId = $state<string | null>(null);
  let editName = $state("");
  let editPrompt = $state("");
  let saving = $state(false);

  let showNew = $state(false);
  let newName = $state("");
  let newPrompt = $state("");
  let previewId = $state<string | null>(null);
  let openTemplateMenu = $state<string | null>(null);

  onMount(() => {
    const dismissMenu = (event: PointerEvent) => {
      if (!(event.target as Element).closest(".action-menu")) openTemplateMenu = null;
    };
    document.addEventListener("pointerdown", dismissMenu);
    void loadFormats();
    return () => document.removeEventListener("pointerdown", dismissMenu);
  });

  async function loadFormats() {
    loading = true;
    try {
      formats = await listNoteFormats();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function startEdit(fmt: NoteFormatTemplate) {
    openTemplateMenu = null;
    editingId = fmt.id;
    editName = fmt.name;
    editPrompt = fmt.prompt;
    showNew = false;
    error = "";
  }

  function cancelEdit() {
    editingId = null;
    editName = "";
    editPrompt = "";
  }

  function uniqueFormatName(base: string) {
    let name = base;
    let suffix = 2;
    while (formats.some((candidate) => candidate.name.toLocaleLowerCase() === name.toLocaleLowerCase())) {
      name = `${base} ${suffix++}`;
    }
    return name;
  }

  async function saveEdit() {
    if (!editingId) return;
    if (!editName.trim()) {
      error = "Name is required";
      return;
    }
    if (!editPrompt.trim()) {
      error = "Prompt is required";
      return;
    }
    saving = true;
    error = "";
    try {
      const original = formats.find((format) => format.id === editingId);
      if (original?.is_builtin) {
        await createNoteFormat(uniqueFormatName(`${editName.trim()} customized`), editPrompt);
      } else {
        await updateNoteFormat(editingId, editName.trim(), editPrompt);
      }
      await loadFormats();
      editingId = null;
      editName = "";
      editPrompt = "";
      saved = true;
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function removeFormat(fmt: NoteFormatTemplate) {
    openTemplateMenu = null;
    if (
      !(await confirm(`Delete note type "${fmt.name}"? This cannot be undone.`, {
        title: "Delete note type",
        kind: "warning",
      }))
    ) return;
    try {
      await deleteNoteFormat(fmt.id);
      await loadFormats();
    } catch (e) {
      error = String(e);
    }
  }

  async function resetFormat(fmt: NoteFormatTemplate) {
    openTemplateMenu = null;
    if (
      !(await confirm(`Reset "${fmt.name}" to the built-in default? Your changes will be discarded.`, {
        title: "Reset note type",
        kind: "warning",
      }))
    ) return;
    try {
      await resetNoteFormat(fmt.id);
      await loadFormats();
      saved = true;
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleHidden(fmt: NoteFormatTemplate) {
    openTemplateMenu = null;
    try {
      await toggleNoteFormatHidden(fmt.id);
      await loadFormats();
    } catch (e) {
      error = String(e);
    }
  }

  async function duplicateFormat(fmt: NoteFormatTemplate) {
    openTemplateMenu = null;
    error = "";
    try {
      await createNoteFormat(uniqueFormatName(`${fmt.name} copy`), fmt.prompt);
      await loadFormats();
      saved = true;
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = String(e);
    }
  }

  const visibleFormats = $derived(formats.filter((f) => !f.hidden));
  const hiddenFormats = $derived(formats.filter((f) => f.hidden));

  function templateDescription(fmt: NoteFormatTemplate) {
    const name = fmt.name.toLowerCase();
    if (name.includes("soap")) return "A concise clinical structure for subjective report, objective observations, assessment, and plan.";
    if (name.includes("dap")) return "A focused format for data, assessment, and the next clinical plan.";
    if (name.includes("birp")) return "A structured record of behavior, intervention, response, and plan.";
    return "A structured clinical note generated from the session’s local source material.";
  }

  async function createFormat() {
    if (!newName.trim()) {
      error = "Name is required";
      return;
    }
    if (!newPrompt.trim()) {
      error = "Prompt is required";
      return;
    }
    saving = true;
    error = "";
    try {
      await createNoteFormat(newName.trim(), newPrompt);
      await loadFormats();
      showNew = false;
      newName = "";
      newPrompt = "";
      saved = true;
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  const PLACEHOLDER_PROMPT = `You are a clinical note-taking assistant for licensed therapists. Generate a {format name} note from labeled clinical source materials.

Rules:
- Source materials may include a session transcript and/or clinician note.
- Base client statements only on the session transcript.
- Use the clinician note for therapist observations, interventions, corrections, clinical context, and plan details.
- ...your rules here...

Output format:

**Section 1:**
- ...`;
</script>

<div class="workspace-header">
  <div class="header-top-row">
    <div>
      <h2>Note templates</h2>
      <div class="header-meta">Choose which note formats are available when creating session notes.</div>
    </div>
    {#if !showNew}
      <button class="btn btn-primary" onclick={() => { showNew = true; newName = ""; newPrompt = ""; }}>Create template</button>
    {/if}
  </div>
</div>

{#if error}
  <div class="error-banner">{error}</div>
{/if}

{#if saved}
  <div class="success-banner">Saved.</div>
{/if}

{#if loading}
  <p class="text-muted">Loading note templates...</p>
{:else}
  <div class="templates-list">
    {#each visibleFormats as fmt (fmt.id)}
      {#if editingId === fmt.id}
        <div class="template-card editing">
          <div class="template-edit-header">
            <input bind:value={editName} placeholder="Note type name" class="template-name-input" />
            <div class="template-edit-actions">
              <button class="btn btn-sm btn-primary" onclick={saveEdit} disabled={saving}>
                {saving ? "Saving..." : "Save"}
              </button>
              <button class="btn btn-sm" onclick={cancelEdit} disabled={saving}>Cancel</button>
            </div>
          </div>
          <details class="advanced-prompt" open>
            <summary>Advanced prompt</summary>
            <p>Changes here can substantially alter the structure and content of generated notes.</p>
            <label class="template-editor-label" for="edit-format-prompt">Generation instructions</label>
            <textarea
              id="edit-format-prompt"
              bind:value={editPrompt}
              class="template-prompt-editor"
              placeholder="Enter the instructions and output structure for this note type..."
            ></textarea>
          </details>
        </div>
      {:else}
        <div class="template-card">
          <div class="template-header">
            <div>
              <div class="template-name">
                {fmt.name}
                {#if fmt.is_builtin}
                <span class="badge badge-blue">Built-in</span>
                {:else}
                <span class="badge">Custom</span>
                {/if}
                <span class="badge badge-active">Active</span>
              </div>
            </div>
            <div class="template-actions">
              <button
                class="btn btn-sm"
                class:preview-active={previewId === fmt.id}
                onclick={() => previewId = previewId === fmt.id ? null : fmt.id}
                aria-pressed={previewId === fmt.id}
              >Preview</button>
              <button class="btn btn-sm" onclick={() => startEdit(fmt)}>{fmt.is_builtin ? "Customize" : "Edit"}</button>
              <div class="action-menu">
                <button
                  class="icon-btn action-menu-trigger"
                  onclick={() => openTemplateMenu = openTemplateMenu === fmt.id ? null : fmt.id}
                  title="Template actions"
                  aria-label="Template actions"
                  aria-expanded={openTemplateMenu === fmt.id}
                >
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <circle cx="5" cy="12" r="1.8"/>
                    <circle cx="12" cy="12" r="1.8"/>
                    <circle cx="19" cy="12" r="1.8"/>
                  </svg>
                </button>
                {#if openTemplateMenu === fmt.id}
                  <div class="action-menu-popover">
                    <button class="action-menu-item" onclick={() => duplicateFormat(fmt)}>Duplicate</button>
                    <button class="action-menu-item" onclick={() => toggleHidden(fmt)}>Hide</button>
                    {#if fmt.is_builtin}
                      <button class="action-menu-item" onclick={() => resetFormat(fmt)}>Reset</button>
                    {:else}
                      <button class="action-menu-item danger" onclick={() => removeFormat(fmt)}>Delete</button>
                    {/if}
                  </div>
                {/if}
              </div>
            </div>
          </div>
          <p class="template-description">{templateDescription(fmt)}</p>
          {#if previewId === fmt.id}
            <div class="template-prompt-preview">
              <strong>Prompt preview</strong>
              <pre>{fmt.prompt}</pre>
            </div>
          {/if}
        </div>
      {/if}
    {/each}
  </div>

  {#if hiddenFormats.length > 0}
    <div class="hidden-section">
      <div class="hidden-section-label">Hidden note types</div>
      <div class="templates-list">
        {#each hiddenFormats as fmt (fmt.id)}
          <div class="template-card hidden">
            <div class="template-header">
              <div>
                <div class="template-name">
                  {fmt.name}
                  {#if fmt.is_builtin}
                    <span class="badge badge-blue">Built-in</span>
                  {:else}
                    <span class="badge">Custom</span>
                  {/if}
                </div>
              </div>
              <div class="template-actions">
                <button class="btn btn-sm" onclick={() => startEdit(fmt)}>{fmt.is_builtin ? "Customize" : "Edit"}</button>
                <div class="action-menu">
                  <button
                    class="icon-btn action-menu-trigger"
                    onclick={() => openTemplateMenu = openTemplateMenu === fmt.id ? null : fmt.id}
                    title="Template actions"
                    aria-label="Template actions"
                    aria-expanded={openTemplateMenu === fmt.id}
                  >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                      <circle cx="5" cy="12" r="1.8"/>
                      <circle cx="12" cy="12" r="1.8"/>
                      <circle cx="19" cy="12" r="1.8"/>
                    </svg>
                  </button>
                  {#if openTemplateMenu === fmt.id}
                    <div class="action-menu-popover">
                      <button class="action-menu-item" onclick={() => duplicateFormat(fmt)}>Duplicate</button>
                      <button class="action-menu-item" onclick={() => toggleHidden(fmt)}>Show</button>
                      {#if fmt.is_builtin}
                        <button class="action-menu-item" onclick={() => resetFormat(fmt)}>Reset</button>
                      {:else}
                        <button class="action-menu-item danger" onclick={() => removeFormat(fmt)}>Delete</button>
                      {/if}
                    </div>
                  {/if}
                </div>
              </div>
            </div>
            <p class="template-description">{templateDescription(fmt)}</p>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if showNew}
    <div class="template-card editing">
      <div class="template-edit-header">
        <input
          bind:value={newName}
          placeholder="Note type name (for example DAP or BIRP)"
          class="template-name-input"
        />
        <div class="template-edit-actions">
          <button class="btn btn-sm btn-primary" onclick={createFormat} disabled={saving}>
            {saving ? "Creating..." : "Create"}
          </button>
          <button
            class="btn btn-sm"
            onclick={() => { showNew = false; newName = ""; newPrompt = ""; }}
            disabled={saving}
          >
            Cancel
          </button>
        </div>
      </div>
      <details class="advanced-prompt" open>
        <summary>Advanced prompt</summary>
        <p>Define the note structure and generation rules. All processing remains local.</p>
        <label class="template-editor-label" for="new-format-prompt">Generation instructions</label>
        <textarea
          id="new-format-prompt"
          bind:value={newPrompt}
          class="template-prompt-editor"
          placeholder={PLACEHOLDER_PROMPT}
        ></textarea>
      </details>
    </div>
  {/if}
{/if}

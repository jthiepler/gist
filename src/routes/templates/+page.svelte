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

  onMount(async () => {
    await loadFormats();
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
      await updateNoteFormat(editingId, editName.trim(), editPrompt);
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
    try {
      await toggleNoteFormatHidden(fmt.id);
      await loadFormats();
    } catch (e) {
      error = String(e);
    }
  }

  const visibleFormats = $derived(formats.filter((f) => !f.hidden));
  const hiddenFormats = $derived(formats.filter((f) => f.hidden));

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
  <h2>Note templates</h2>
  <div class="header-meta">Choose which note types are available when creating sessions.</div>
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
          <label class="template-editor-label" for="edit-format-prompt">Generation instructions</label>
          <textarea
            id="edit-format-prompt"
            bind:value={editPrompt}
            class="template-prompt-editor"
            placeholder="Enter the instructions and output structure for this note type..."
          ></textarea>
        </div>
      {:else}
        <div class="template-card">
          <div class="template-header">
            <div>
              <div class="template-name">
                {fmt.name.toUpperCase()}
                {#if fmt.is_builtin}
                <span class="badge badge-blue">Built-in</span>
                {/if}
              </div>
            </div>
            <div class="template-actions">
              <button class="btn-ghost btn-sm" onclick={() => startEdit(fmt)}>Edit</button>
              <button class="btn-ghost btn-sm" onclick={() => toggleHidden(fmt)}>Hide</button>
              {#if fmt.is_builtin}
                <button class="btn-ghost btn-sm" onclick={() => resetFormat(fmt)}>Reset</button>
              {:else}
                <button class="btn btn-sm btn-danger" onclick={() => removeFormat(fmt)}>Delete</button>
              {/if}
            </div>
          </div>
          <div class="template-preview">{fmt.prompt.slice(0, 200)}{#if fmt.prompt.length > 200}...{/if}</div>
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
                  {fmt.name.toUpperCase()}
                  {#if fmt.is_builtin}
                    <span class="badge badge-blue">Built-in</span>
                  {/if}
                </div>
              </div>
              <div class="template-actions">
                <button class="btn-ghost btn-sm" onclick={() => startEdit(fmt)}>Edit</button>
                <button class="btn-ghost btn-sm" onclick={() => toggleHidden(fmt)}>Show</button>
                {#if fmt.is_builtin}
                  <button class="btn-ghost btn-sm" onclick={() => resetFormat(fmt)}>Reset</button>
                {:else}
                  <button class="btn btn-sm btn-danger" onclick={() => removeFormat(fmt)}>Delete</button>
                {/if}
              </div>
            </div>
            <div class="template-preview">{fmt.prompt.slice(0, 200)}{#if fmt.prompt.length > 200}...{/if}</div>
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
      <label class="template-editor-label" for="new-format-prompt">Generation instructions</label>
      <textarea
        id="new-format-prompt"
        bind:value={newPrompt}
        class="template-prompt-editor"
        placeholder={PLACEHOLDER_PROMPT}
      ></textarea>
    </div>
  {:else}
    <button class="btn btn-primary add-format-btn" onclick={() => { showNew = true; newName = ""; newPrompt = ""; }}>
      New note type
    </button>
  {/if}
{/if}

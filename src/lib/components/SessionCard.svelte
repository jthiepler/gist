<script lang="ts">
  import { marked } from "marked";
  import type { Session } from "$lib/types";

  let {
    session,
    isGenerating = false,
    onGenerateNote,
    onDelete,
  }: {
    session: Session;
    isGenerating?: boolean;
    onGenerateNote: (session: Session) => void;
    onDelete: (session: Session) => void;
  } = $props();

  let expanded = $state(true);

  // Sort notes alphabetically by format
  let sortedNotes = $derived(
    [...session.notes].sort((a, b) => a.format.localeCompare(b.format))
  );

  // Tabs: one per note format (alphabetical), then "transcript" at end
  let tabs = $derived([
    ...sortedNotes.map((n) => ({ key: n.format, label: n.format.toUpperCase(), type: "note" as const, note: n })),
    { key: "transcript", label: "Transcript", type: "transcript" as const, note: null },
  ]);

  let activeTab = $derived.by(() => {
    // Default: first note if available, otherwise transcript
    if (sortedNotes.length > 0) return sortedNotes[0].format;
    return "transcript";
  });

  let currentTab = $state<string | null>(null);
  let activeKey = $derived(currentTab ?? activeTab);

  // Reset currentTab when session changes
  $effect(() => {
    const _ = session.id;
    currentTab = null;
  });

  let formattedDate = $derived(
    new Date(session.date).toLocaleDateString("en-US", {
      year: "numeric",
      month: "long",
      day: "numeric",
    })
  );

  let durationMin = $derived(
    session.duration_seconds ? Math.round(session.duration_seconds / 60) : null
  );

  let activeNote = $derived(sortedNotes.find((n) => n.format === activeKey));

  let renderedNote = $derived(
    activeNote?.note ? marked.parse(activeNote.note, { breaks: true }) : ""
  );

  function toggle() {
    expanded = !expanded;
  }
</script>

<div class="session-card" class:generating={isGenerating} class:collapsed={!expanded}>
  <div class="session-card-header" onclick={toggle} role="button" tabindex="0"
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(); } }}>
    <div class="session-header-left">
      <svg class="session-chevron" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="9 18 15 12 9 6"/>
      </svg>
      <div class="session-date">{formattedDate}</div>
    </div>
    <div class="session-meta">
      {#each sortedNotes as n}
        <span class="badge badge-blue">{n.format.toUpperCase()}</span>
      {/each}
      {#if durationMin}
        <span>{durationMin} min</span>
      {/if}
      {#if session.language}
        <span>{session.language}</span>
      {/if}
      <button class="btn btn-sm btn-danger delete-session-btn" onclick={(e) => { e.stopPropagation(); onDelete(session); }} disabled={isGenerating}>Delete</button>
    </div>
  </div>

  {#if expanded}
    <div class="session-card-tabs">
      {#each tabs as tab (tab.key)}
        <button
          class="session-tab"
          class:active={activeKey === tab.key}
          onclick={() => currentTab = tab.key}
          disabled={isGenerating || (tab.type === "transcript" && !session.transcript)}
        >
          {tab.label}
        </button>
      {/each}
    </div>

    <div class="session-card-body">
      {#if isGenerating}
        <div class="spinner-container">
          <div class="spinner"></div>
          <p class="text-muted spinner-message">Generating note...</p>
        </div>
      {:else if activeKey === "transcript"}
        {#if session.transcript}
          <div class="session-content">{session.transcript}</div>
        {:else}
          <p class="text-muted">No transcript available.</p>
        {/if}
      {:else if activeNote}
        {#if activeNote.note}
          <div class="markdown-content">{@html renderedNote}</div>
        {:else}
          <div class="spinner-container">
            <p class="text-muted empty-message">No {activeNote.format.toUpperCase()} note generated yet.</p>
            <button class="btn btn-primary" onclick={() => onGenerateNote(session)}>
              Generate Note
            </button>
          </div>
        {/if}
      {:else if session.transcript}
        <div class="spinner-container">
          <p class="text-muted empty-message">No notes generated yet.</p>
          <button class="btn btn-primary" onclick={() => onGenerateNote(session)}>
            Generate Notes
          </button>
        </div>
      {:else}
        <p class="text-muted">No content available.</p>
      {/if}
    </div>
  {/if}
</div>

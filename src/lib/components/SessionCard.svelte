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

  // Default to "note" tab, but if no note, show "transcript" tab
  let activeTab = $state<"note" | "transcript">(session.note ? "note" : "transcript");

  // Reset tab when session changes
  $effect(() => {
    activeTab = session.note ? "note" : "transcript";
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

  let renderedNote = $derived(
    session.note ? marked.parse(session.note, { breaks: true }) : ""
  );
</script>

<div class="session-card" class:generating={isGenerating}>
  <div class="session-card-header">
    <div class="session-date">{formattedDate}</div>
    <div class="session-meta">
      {#if session.note_format}
        <span class="badge badge-blue">{session.note_format.toUpperCase()}</span>
      {/if}
      {#if durationMin}
        <span>{durationMin} min</span>
      {/if}
      {#if session.language}
        <span>{session.language}</span>
      {/if}
      <button class="btn-ghost btn-sm delete-session-btn" onclick={() => onDelete(session)} disabled={isGenerating}>Delete</button>
    </div>
  </div>

  <div class="session-card-tabs">
    <button
      class="session-tab"
      class:active={activeTab === "note"}
      onclick={() => activeTab = "note"}
      disabled={(!session.note && !session.transcript) || isGenerating}
    >
      Note
    </button>
    <button
      class="session-tab"
      class:active={activeTab === "transcript"}
      onclick={() => activeTab = "transcript"}
      disabled={!session.transcript || isGenerating}
    >
      Transcript
    </button>
  </div>

  <div class="session-card-body">
    {#if isGenerating}
      <div class="spinner-container">
        <div class="spinner"></div>
        <p class="text-muted spinner-message">Generating note...</p>
      </div>
    {:else if activeTab === "note"}
      {#if session.note}
        <div class="markdown-content">{@html renderedNote}</div>
      {:else if session.transcript}
        <div class="spinner-container">
          <p class="text-muted empty-message">No note generated yet.</p>
          <button class="btn btn-primary" onclick={() => onGenerateNote(session)}>
            Generate Note
          </button>
        </div>
      {:else}
        <p class="text-muted">No content available.</p>
      {/if}
    {:else}
      {#if session.transcript}
        <div class="session-content">{session.transcript}</div>
      {:else}
        <p class="text-muted">No transcript available.</p>
      {/if}
    {/if}
  </div>
</div>

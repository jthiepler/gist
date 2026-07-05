<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/stores";
  import type { Patient, Session } from "$lib/types";
  import Card from "$lib/components/Card.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";

  let patient = $state<Patient | null>(null);
  let sessions = $state<Session[]>([]);
  let loading = $state(true);
  let error = $state("");

  const patientId = $derived($page.params.id);

  // Re-fetch when the route param changes (SvelteKit reuses the component)
  $effect(() => {
    const id = patientId;
    loading = true;
    error = "";
    patient = null;
    sessions = [];

    (async () => {
      try {
        const all = await invoke<Patient[]>("list_patients");
        patient = all.find((p) => p.id === id) ?? null;
        if (!patient) {
          error = "Patient not found.";
        } else {
          sessions = await invoke<Session[]>("list_sessions", { patientId: id });
        }
      } catch (e) {
        error = String(e);
      } finally {
        loading = false;
      }
    })();
  });

  async function removeSession(id: string) {
    if (!confirm("Delete this session?")) return;
    try {
      await invoke("delete_session", { id });
      sessions = sessions.filter((s) => s.id !== id);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="page-header">
  <h2>{patient?.name ?? "Patient"}</h2>
  <p><a href="/patients" style="color: var(--accent);">← Back to Patients</a></p>
</div>

{#if error}
  <div class="card" style="border-color: var(--error);">
    <p style="color: var(--error); font-size: 13px;">{error}</p>
  </div>
{/if}

<Card title="Sessions">
  {#if loading}
    <p style="color: var(--text-muted); font-size: 13px; padding: 16px;">Loading...</p>
  {:else if sessions.length === 0}
    <EmptyState icon="📅" text="No sessions recorded for this patient." />
  {:else}
    <ul class="item-list">
      {#each sessions as session}
        <li>
          <a href="/sessions/{session.id}" class="item-main" style="text-decoration: none;">
            <strong>{session.date}</strong>
            <span class="item-meta">
              {session.duration_seconds ? `${Math.round(session.duration_seconds / 60)} min` : '—'}
              {#if session.note}
                · <span class="badge badge-success">Note ready</span>
              {/if}
              {#if session.transcript}
                · Transcript saved
              {/if}
            </span>
          </a>
          <div class="item-actions">
            <button class="btn btn-sm btn-danger" onclick={() => removeSession(session.id)}>Delete</button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</Card>

<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Patient, Session } from "$lib/types";
  import { patientMap } from "$lib/stores";
  import Card from "$lib/components/Card.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";

  let sessions = $state<Session[]>([]);
  let loading = $state(true);
  let error = $state("");

  onMount(async () => {
    try {
      // Fetch patients first to build the lookup map
      const patients = await invoke<Patient[]>("list_patients");
      const pmap: Record<string, Patient> = {};
      for (const p of patients) pmap[p.id] = p;
      patientMap.set(pmap);

      sessions = await invoke<Session[]>("list_sessions");
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
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

  function patientName(id: string): string {
    return $patientMap[id]?.name ?? id.slice(0, 8);
  }
</script>

<div class="page-header">
  <h2>Sessions</h2>
  <p>All recorded sessions</p>
</div>

{#if error}
  <div class="card" style="border-color: var(--error);">
    <p style="color: var(--error); font-size: 13px;">{error}</p>
  </div>
{/if}

<Card title="All Sessions">
  {#if loading}
    <p style="color: var(--text-muted); font-size: 13px; padding: 16px;">Loading...</p>
  {:else if sessions.length === 0}
    <EmptyState icon="📅" text="No sessions yet. Transcribe an audio file and save it as a session." />
  {:else}
    <ul class="item-list">
      {#each sessions as session}
        <li>
          <a href="/sessions/{session.id}" class="item-main" style="text-decoration: none;">
            <strong>{session.date}</strong>
            <span class="item-meta">
              {patientName(session.patient_id)}
              {#if session.duration_seconds}
                · {Math.round(session.duration_seconds / 60)} min
              {/if}
              {#if session.note}
                · <span class="badge badge-success">Note ready</span>
              {/if}
              {#if session.transcript}
                · {session.transcript.length} chars
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

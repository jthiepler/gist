<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { startSidecar } from "$lib/rpc";
  import { sidecarRunning, patientMap } from "$lib/stores";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import type { Patient, Session } from "$lib/types";
  import Card from "$lib/components/Card.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";

  let patients = $state<Patient[]>([]);
  let recentSessions = $state<Session[]>([]);
  let loading = $state(true);
  let error = $state("");

  onMount(async () => {
    try {
      patients = await invoke<Patient[]>("list_patients");
      const pmap: Record<string, Patient> = {};
      for (const p of patients) pmap[p.id] = p;
      patientMap.set(pmap);

      recentSessions = await invoke<Session[]>("list_sessions");
      recentSessions = recentSessions.slice(0, 5);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function handleStart() {
    const ok = await ensureSidecar();
    if (!ok) error = "Failed to start sidecar. Check that the sidecar binary exists.";
  }

  function patientName(id: string): string {
    return $patientMap[id]?.name ?? id.slice(0, 8);
  }
</script>

<div class="page-header">
  <h2>Dashboard</h2>
  <p>Welcome to Gist — your local-first therapy notes tool</p>
</div>

{#if error}
  <div class="card" style="border-color: var(--error);">
    <p style="color: var(--error); font-size: 13px;">{error}</p>
  </div>
{/if}

<Card title="Sidecar">
  <div class="toggle-row">
    <div>
      <p style="font-size: 13px;">The Python backend that runs transcription and note generation.</p>
    </div>
    {#if $sidecarRunning}
      <span class="badge badge-success">Running</span>
    {:else}
      <button class="btn btn-primary" onclick={handleStart}>Start Sidecar</button>
    {/if}
  </div>
</Card>

<div style="display: grid; grid-template-columns: 1fr 1fr; gap: 16px;">
  <Card title="Patients ({patients.length})">
    {#if loading}
      <p style="color: var(--text-muted); font-size: 13px;">Loading...</p>
    {:else if patients.length === 0}
      <EmptyState icon="👤" text="No patients yet. Add your first patient." />
    {:else}
      <ul class="item-list">
        {#each patients.slice(0, 5) as patient}
          <li>
            <a href="/patients/{patient.id}" class="item-main" style="text-decoration: none;">
              <strong>{patient.name}</strong>
              <span class="item-meta">Added {patient.created_at.slice(0, 10)}</span>
            </a>
          </li>
        {/each}
      </ul>
      {#if patients.length > 5}
        <p style="padding: 8px 16px; font-size: 12px;">
          <a href="/patients">View all {patients.length} patients →</a>
        </p>
      {/if}
    {/if}
  </Card>

  <Card title="Recent Sessions">
    {#if loading}
      <p style="color: var(--text-muted); font-size: 13px;">Loading...</p>
    {:else if recentSessions.length === 0}
      <EmptyState icon="📅" text="No sessions yet. Transcribe an audio file to begin." />
    {:else}
      <ul class="item-list">
        {#each recentSessions as session}
          <li>
            <a href="/sessions/{session.id}" class="item-main" style="text-decoration: none;">
              <strong>{session.date}</strong>
              <span class="item-meta">
                {patientName(session.patient_id)}
                {#if session.duration_seconds}
                  · {Math.round(session.duration_seconds / 60)} min
                {/if}
                {#if session.note}
                  · <span class="badge badge-success">Note</span>
                {/if}
              </span>
            </a>
          </li>
        {/each}
      </ul>
      <p style="padding: 8px 16px; font-size: 12px;">
        <a href="/sessions">View all sessions →</a>
      </p>
    {/if}
  </Card>
</div>

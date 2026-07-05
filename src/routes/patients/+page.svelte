<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Patient } from "$lib/types";
  import Card from "$lib/components/Card.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";

  let patients = $state<Patient[]>([]);
  let loading = $state(true);
  let showForm = $state(false);
  let newName = $state("");
  let error = $state("");

  onMount(async () => {
    try {
      patients = await invoke<Patient[]>("list_patients");
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function addPatient() {
    if (!newName.trim()) return;
    try {
      const created = await invoke<Patient>("create_patient", { data: { name: newName.trim() } });
      patients = [created, ...patients];
      newName = "";
      showForm = false;
    } catch (e) {
      error = String(e);
    }
  }

  async function removePatient(id: string) {
    if (!confirm("Delete this patient and all their sessions?")) return;
    try {
      await invoke("delete_patient", { id });
      patients = patients.filter((p) => p.id !== id);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="page-header">
  <h2>Patients</h2>
  <p>Manage your patient roster</p>
</div>

{#if error}
  <div class="card" style="border-color: var(--error);">
    <p style="color: var(--error); font-size: 13px;">{error}</p>
  </div>
{/if}

<div style="margin-bottom: 16px;">
  {#if showForm}
    <Card title="New Patient">
      <div class="form-group">
        <label for="name">Patient Name</label>
        <input id="name" bind:value={newName} placeholder="e.g. Jane Doe" onkeydown={(e) => e.key === 'Enter' && addPatient()} />
      </div>
      <div style="display: flex; gap: 8px;">
        <button class="btn btn-primary" onclick={addPatient} disabled={!newName.trim()}>Add Patient</button>
        <button class="btn" onclick={() => { showForm = false; newName = ''; }}>Cancel</button>
      </div>
    </Card>
  {:else}
    <button class="btn btn-primary" onclick={() => showForm = true}>+ Add Patient</button>
  {/if}
</div>

<Card title="All Patients">
  {#if loading}
    <p style="color: var(--text-muted); font-size: 13px; padding: 16px;">Loading...</p>
  {:else if patients.length === 0}
    <EmptyState icon="👤" text="No patients yet. Click 'Add Patient' to create one." />
  {:else}
    <ul class="item-list">
      {#each patients as patient}
        <li>
          <a href="/patients/{patient.id}" class="item-main" style="text-decoration: none;">
            <strong>{patient.name}</strong>
            <span class="item-meta">Added {patient.created_at.slice(0, 10)}</span>
          </a>
          <div class="item-actions">
            <button class="btn btn-sm btn-danger" onclick={() => removePatient(patient.id)}>Delete</button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</Card>

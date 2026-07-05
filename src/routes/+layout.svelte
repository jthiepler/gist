<script lang="ts">
  import "../app.css";
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { isRunning, startSidecar, onProgress } from "$lib/rpc";
  import { patients, selectedPatientId, sidecarRunning, progressPercent, progressStage } from "$lib/stores";
  import { page } from "$app/stores";
  import type { Patient } from "$lib/types";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  let { children } = $props();

  let unlisten: UnlistenFn | null = null;
  let showAddForm = $state(false);
  let newName = $state("");
  let addError = $state("");

  onMount(async () => {
    // Auto-start sidecar silently
    try {
      const running = await isRunning();
      if (running) {
        sidecarRunning.set(true);
      } else {
        await startSidecar();
        sidecarRunning.set(true);
      }
    } catch (e) {
      console.error("Failed to auto-start sidecar:", e);
    }

    // Load patients
    try {
      const list = await invoke<Patient[]>("list_patients");
      patients.set(list);
    } catch (e) {
      console.error("Failed to load patients:", e);
    }

    // Global progress listener
    unlisten = await onProgress((data) => {
      progressPercent.set(data.percent);
      progressStage.set(data.stage);
    });
  });

  onDestroy(() => {
    unlisten?.();
  });

  // Sync selectedPatientId from URL
  $effect(() => {
    const p = $page.url.pathname;
    const match = p.match(/^\/patients\/(.+)$/);
    selectedPatientId.set(match ? match[1] : null);
  });

  async function addPatient() {
    if (!newName.trim()) return;
    addError = "";
    try {
      const created = await invoke<Patient>("create_patient", { data: { name: newName.trim() } });
      patients.update((list) => [created, ...list]);
      newName = "";
      showAddForm = false;
    } catch (e) {
      addError = String(e);
    }
  }

  let pathname = $derived($page.url.pathname);
  const isSettings = $derived(pathname === "/settings");
</script>

<div class="app-shell">
  <aside class="sidebar">
    <div class="sidebar-header">
      <h1>Gist</h1>
    </div>

    <div class="sidebar-section-label">Patients</div>

    <div class="patient-list">
      {#each $patients as patient (patient.id)}
        <a
          href="/patients/{patient.id}"
          class="patient-item"
          class:active={$selectedPatientId === patient.id}
        >
          <span class="patient-name">{patient.name}</span>
        </a>
      {/each}
    </div>

    <div class="sidebar-footer">
      {#if showAddForm}
        <div class="add-patient-form">
          <input
            bind:value={newName}
            placeholder="Patient name"
            onkeydown={(e) => {
              if (e.key === "Enter") addPatient();
              if (e.key === "Escape") { showAddForm = false; newName = ""; }
            }}
          />
          {#if addError}
            <p style="color: var(--error); font-size: 11px; margin-top: 4px;">{addError}</p>
          {/if}
          <div class="form-actions">
            <button class="btn btn-sm btn-primary" onclick={addPatient} disabled={!newName.trim()}>Add</button>
            <button class="btn btn-sm" onclick={() => { showAddForm = false; newName = ""; }}>Cancel</button>
          </div>
        </div>
      {:else}
        <button class="add-patient-btn" onclick={() => showAddForm = true}>
          <span>+</span>
          <span>Add Patient</span>
        </button>
      {/if}

      <a href="/settings" class="settings-link" class:active={isSettings}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="3"/>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>
        </svg>
      </a>
    </div>
  </aside>

  <main class="main-content">
    {@render children()}
  </main>
</div>

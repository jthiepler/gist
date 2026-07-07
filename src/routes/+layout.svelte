<script lang="ts">
  import "../app.css";
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { isRunning, startSidecar, onProgress, cancelSidecar } from "$lib/rpc";
  import { patients, selectedPatientId, sidecarRunning, progressPercent, progressStage, progressEta, progressBase, progressScale, darkMode, sidecarBusy, activeOperation } from "$lib/stores";
  import { get } from "svelte/store";
  import { loadDarkMode } from "$lib/settings";
  import { page } from "$app/stores";
  import type { Patient } from "$lib/types";

  let { children } = $props();

  let unlistenProgress: UnlistenFn | null = null;
  let unlistenState: UnlistenFn | null = null;
  let showAddForm = $state(false);
  let newName = $state("");
  let addError = $state("");

  function formatEta(seconds: number): string {
    if (seconds < 60) return `${Math.round(seconds)}s`;
    if (seconds < 3600) {
      const m = Math.floor(seconds / 60);
      const s = Math.round(seconds % 60);
      return s > 0 ? `${m}m ${s}s` : `${m}m`;
    }
    const h = Math.floor(seconds / 3600);
    const m = Math.round((seconds % 3600) / 60);
    return m > 0 ? `${h}h ${m}m` : `${h}h`;
  }

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
    unlistenProgress = await onProgress((data) => {
      const base = get(progressBase);
      const scale = get(progressScale);
      progressPercent.set(base + Math.round((data.percent / 100) * scale));
      progressStage.set(data.stage);
      progressEta.set(data.eta_seconds ?? null);
    });

    // Sidecar busy state listener
    unlistenState = await listen<{ busy: boolean }>("sidecar-state", (event) => {
      sidecarBusy.set(event.payload.busy);
      if (!event.payload.busy) {
        activeOperation.set({ type: null, label: "" });
        progressPercent.set(0);
        progressStage.set("");
        progressEta.set(null);
        progressBase.set(0);
        progressScale.set(100);
      }
    });

    // Load dark mode setting
    await loadDarkMode();
  });

  // Apply dark mode class to <html>
  $effect(() => {
    if ($darkMode) {
      document.documentElement.classList.add("dark");
    } else {
      document.documentElement.classList.remove("dark");
    }
  });

  onDestroy(() => {
    unlistenProgress?.();
    unlistenState?.();
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

  async function handleCancel() {
    try {
      await cancelSidecar();
    } catch (e) {
      console.error("Cancel failed:", e);
    }
  }

  let pathname = $derived($page.url.pathname);
  const isSettings = $derived(pathname === "/settings");
  const isTemplates = $derived(pathname === "/templates");
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

      <a href="/templates" class="footer-link" class:active={isTemplates}>Templates</a>
      <a href="/settings" class="footer-link" class:active={isSettings}>Settings</a>
    </div>
  </aside>

  <main class="main-content">
    {@render children()}
  </main>
</div>

{#if $sidecarBusy}
  <div class="progress-card">
    <div class="progress-card-header">
      <span class="progress-card-title">
        {$activeOperation.label || $progressStage || "Working..."}
      </span>
      <button class="progress-card-cancel" onclick={handleCancel}>Cancel</button>
    </div>
    <div class="progress-bar">
      <div class="progress-bar-fill" style="width: {$progressPercent}%;"></div>
    </div>
    {#if $progressEta != null && $progressEta > 0}
      <div class="progress-card-eta">~{formatEta($progressEta)} remaining</div>
    {/if}
  </div>
{/if}

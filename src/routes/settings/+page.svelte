<script lang="ts">
  import { onMount } from "svelte";
  import {
    listModels, downloadModel, deleteModel,
    startSidecar, stopSidecar, setSetting,
  } from "$lib/rpc";
  import { sidecarRunning, progressPercent, progressStage, darkMode, sidecarBusy, activeOperation } from "$lib/stores";
  import { loadSettings, saveSettings } from "$lib/settings";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import type { ModelsResult } from "$lib/types";

  let models = $state<ModelsResult | null>(null);
  let downloading = $state("");
  let deleting = $state("");
  let thinking = $state(false);
  let selectedLlm = $state("");
  let selectedTranscription = $state("");
  let error = $state("");
  let saved = $state(false);
  let showDebug = $state(false);

  async function refreshModels() {
    models = await listModels();
  }

  onMount(async () => {
    const ok = await ensureSidecar();
    if (!ok) {
      error = "Failed to start the processing engine.";
      return;
    }

    try {
      await refreshModels();
      if (models && Object.keys(models.llm).length > 0) {
        selectedLlm = Object.keys(models.llm)[0];
      }
      if (models && Object.keys(models.transcription).length > 0) {
        selectedTranscription = Object.keys(models.transcription)[0];
      }
    } catch (e) {
      console.error("Failed to load models:", e);
      error = "Failed to load models.";
    }

    const s = await loadSettings();
    if (s.defaultLlm) selectedLlm = s.defaultLlm;
    if (s.defaultTranscription) selectedTranscription = s.defaultTranscription;
    thinking = s.thinking;
    darkMode.set(s.darkMode);
  });

  async function handleDownload(model: string, kind: string) {
    if ($sidecarBusy) {
      error = "Another operation is in progress. Please wait or cancel it first.";
      return;
    }
    downloading = model;
    progressPercent.set(0);
    progressStage.set("Starting download...");
    activeOperation.set({ type: "download_model", label: `Downloading ${model}...` });

    try {
      await downloadModel(model, kind);
      await refreshModels();
      saved = true;
      setTimeout(() => saved = false, 3000);
    } catch (e) {
      const msg = String(e);
      if (msg === "sidecar_busy") {
        error = "Another operation is in progress. Please wait or cancel it first.";
      } else {
        error = `Download failed: ${msg}`;
      }
    } finally {
      downloading = "";
      progressPercent.set(0);
      progressStage.set("");
      activeOperation.set({ type: null, label: "" });
    }
  }

  async function handleDelete(model: string, kind: string) {
    if ($sidecarBusy) {
      error = "Another operation is in progress. Please wait or cancel it first.";
      return;
    }
    deleting = model;
    try {
      await deleteModel(model, kind);
      await refreshModels();
    } catch (e) {
      const msg = String(e);
      if (msg === "sidecar_busy") {
        error = "Another operation is in progress. Please wait or cancel it first.";
      } else {
        error = `Delete failed: ${msg}`;
      }
    } finally {
      deleting = "";
    }
  }

  function selectModel(name: string, kind: "llm" | "transcription") {
    if (kind === "llm") selectedLlm = name;
    else selectedTranscription = name;
  }

  async function savePreferences() {
    try {
      await saveSettings({
        defaultLlm: selectedLlm,
        defaultTranscription: selectedTranscription,
        thinking,
        darkMode: $darkMode,
      });
      saved = true;
      setTimeout(() => saved = false, 3000);
    } catch (e) {
      error = String(e);
    }
  }

  async function handleRestart() {
    try {
      await stopSidecar();
      sidecarRunning.set(false);
      await startSidecar();
      sidecarRunning.set(true);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="workspace-header">
  <h2>Settings</h2>
</div>

{#if error}
  <div class="error-banner">{error}</div>
{/if}

{#if saved}
  <div class="success-banner">Saved.</div>
{/if}

<!-- Models -->
<div class="settings-section">
  <h3>Models</h3>
  <p class="text-muted" style="font-size: 13px; margin-bottom: 20px;">
    Click a downloaded model to select it as the default.
  </p>

  {#if models}
    <div style="margin-bottom: 28px;">
      <div class="settings-row" style="border-bottom: 1px solid var(--border-subtle); padding-bottom: 8px; margin-bottom: 8px;">
        <strong style="font-size: 13px; color: var(--text-muted);">LLM</strong>
      </div>
      <table class="model-table">
        <thead>
          <tr>
            <th style="width: 30%;">Name</th>
            <th>Description</th>
            <th style="width: 60px;">Size</th>
            <th style="width: 100px;"></th>
          </tr>
        </thead>
        <tbody>
          {#each Object.entries(models.llm) as [name, info]}
            <tr
              class="model-row {info.downloaded ? 'model-available' : 'model-not-downloaded'}"
              class:model-selected={selectedLlm === name}
              onclick={() => info.downloaded && selectModel(name, "llm")}
            >
              <td>{info.display}</td>
              <td class="model-desc">{info.description}</td>
              <td>{info.size_gb} GB</td>
              <td>
                {#if info.downloaded}
                  {#if selectedLlm === name}
                    <span class="model-selected-marker">✓ Selected</span>
                  {:else}
                    <button
                      class="btn btn-sm btn-danger"
                      onclick={(e) => { e.stopPropagation(); handleDelete(name, "llm"); }}
                      disabled={deleting === name}
                    >
                      {deleting === name ? "..." : "Delete"}
                    </button>
                  {/if}
                {:else}
                  <button
                    class="btn btn-sm btn-primary"
                    onclick={(e) => { e.stopPropagation(); handleDownload(name, "llm"); }}
                    disabled={downloading === name}
                  >
                    {downloading === name ? "..." : "Download"}
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div>
      <div class="settings-row" style="border-bottom: 1px solid var(--border-subtle); padding-bottom: 8px; margin-bottom: 8px;">
        <strong style="font-size: 13px; color: var(--text-muted);">Transcription</strong>
      </div>
      <table class="model-table">
        <thead>
          <tr>
            <th style="width: 30%;">Name</th>
            <th>Description</th>
            <th style="width: 60px;">Size</th>
            <th style="width: 100px;"></th>
          </tr>
        </thead>
        <tbody>
          {#each Object.entries(models.transcription) as [name, info]}
            <tr
              class="model-row {info.downloaded ? 'model-available' : 'model-not-downloaded'}"
              class:model-selected={selectedTranscription === name}
              onclick={() => info.downloaded && selectModel(name, "transcription")}
            >
              <td>{info.display}</td>
              <td class="model-desc">{info.description}</td>
              <td>{info.size_gb} GB</td>
              <td>
                {#if info.downloaded}
                  {#if selectedTranscription === name}
                    <span class="model-selected-marker">✓ Selected</span>
                  {:else}
                    <button
                      class="btn btn-sm btn-danger"
                      onclick={(e) => { e.stopPropagation(); handleDelete(name, "transcription"); }}
                      disabled={deleting === name}
                    >
                      {deleting === name ? "..." : "Delete"}
                    </button>
                  {/if}
                {:else}
                  <button
                    class="btn btn-sm btn-primary"
                    onclick={(e) => { e.stopPropagation(); handleDownload(name, "transcription"); }}
                    disabled={downloading === name}
                  >
                    {downloading === name ? "..." : "Download"}
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <p class="text-muted">Loading models...</p>
  {/if}
</div>

<!-- Preferences -->
<div class="settings-section">
  <h3>Preferences</h3>

  <div class="settings-row">
    <div>
      <div class="setting-label">Reasoning (Thinking)</div>
      <div class="setting-desc">Enables extended reasoning before generating the note</div>
    </div>
    <div class="toggle" class:active={thinking} onclick={() => thinking = !thinking}>
      <div class="toggle-knob"></div>
    </div>
  </div>

  <div class="settings-row">
    <div>
      <div class="setting-label">Dark Mode</div>
      <div class="setting-desc">Toggle between light and dark appearance</div>
    </div>
    <div class="toggle" class:active={$darkMode} onclick={async () => {
      darkMode.set(!$darkMode);
      try { await setSetting("dark_mode", String(!$darkMode)); } catch (e) { console.error("Failed to persist dark mode:", e); }
    }}>
      <div class="toggle-knob"></div>
    </div>
  </div>

  <div style="margin-top: 16px;">
    <button class="btn btn-primary" onclick={savePreferences}>Save Preferences</button>
  </div>
</div>

<!-- Debug -->
<div class="settings-section">
  <div class="debug-section">
    <button class="debug-toggle" onclick={() => showDebug = !showDebug}>
      <span>Advanced</span>
      <span>{showDebug ? "▾" : "▸"}</span>
    </button>

    {#if showDebug}
      <div class="debug-content">
        <div class="debug-row">
          <span class="status-dot" class:running={$sidecarRunning} class:stopped={!$sidecarRunning}></span>
          <span>Engine: {$sidecarRunning ? "Running" : "Stopped"}</span>
          <button class="btn btn-sm" onclick={handleRestart} style="margin-left: auto;">Restart</button>
        </div>
      </div>
    {/if}
  </div>
</div>

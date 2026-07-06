<script lang="ts">
  import { onMount } from "svelte";
  import {
    listModels, downloadModel,
    isRunning, startSidecar, stopSidecar,
  } from "$lib/rpc";
  import { sidecarRunning, progressPercent, progressStage, darkMode } from "$lib/stores";
  import { loadSettings, saveSettings } from "$lib/settings";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import type { ModelsResult } from "$lib/types";

  let models = $state<ModelsResult | null>(null);
  let downloading = $state("");
  let defaultFormat = $state("soap");
  let thinking = $state(false);
  let selectedLlm = $state("");
  let selectedTranscription = $state("");
  let error = $state("");
  let saved = $state(false);
  let showDebug = $state(false);

  onMount(async () => {
    const ok = await ensureSidecar();
    if (!ok) {
      error = "Failed to start the processing engine.";
      return;
    }

    try {
      models = await listModels();
      if (models && Object.keys(models.llm).length > 0) {
        selectedLlm = Object.keys(models.llm)[0];
      }
      if (models && Object.keys(models.transcription).length > 0) {
        selectedTranscription = Object.keys(models.transcription)[0];
      }
    } catch {}

    const s = await loadSettings();
    if (s.defaultFormat) defaultFormat = s.defaultFormat;
    if (s.defaultLlm) selectedLlm = s.defaultLlm;
    if (s.defaultTranscription) selectedTranscription = s.defaultTranscription;
    thinking = s.thinking;
    darkMode.set(s.darkMode);
  });

  async function handleDownload(model: string, kind: string) {
    downloading = model;
    progressPercent.set(0);
    progressStage.set("Starting download...");

    try {
      await downloadModel(model, kind);
      saved = true;
      setTimeout(() => saved = false, 3000);
    } catch (e) {
      error = `Download failed: ${e}`;
    } finally {
      downloading = "";
      progressPercent.set(0);
      progressStage.set("");
    }
  }

  async function savePreferences() {
    try {
      await saveSettings({
        defaultFormat,
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

  {#if models}
    <div style="margin-bottom: 24px;">
      <div class="settings-row" style="border-bottom: 1px solid var(--border-subtle); padding-bottom: 8px; margin-bottom: 8px;">
        <strong style="font-size: 13px; color: var(--text-muted);">LLM</strong>
      </div>
      <table class="model-table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Backend</th>
            <th>Size</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each Object.entries(models.llm) as [name, info]}
            <tr>
              <td>{info.display}</td>
              <td>{info.backend}</td>
              <td>{info.size_gb} GB</td>
              <td>
                <button
                  class="btn btn-sm"
                  onclick={() => handleDownload(name, "llm")}
                  disabled={downloading === name}
                >
                  {downloading === name ? "Downloading..." : "Download"}
                </button>
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
            <th>Name</th>
            <th>Backend</th>
            <th>Size</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each Object.entries(models.transcription) as [name, info]}
            <tr>
              <td>{info.display}</td>
              <td>{info.backend}</td>
              <td>{info.size_gb} GB</td>
              <td>
                <button
                  class="btn btn-sm"
                  onclick={() => handleDownload(name, "transcription")}
                  disabled={downloading === name}
                >
                  {downloading === name ? "Downloading..." : "Download"}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if downloading}
      <div style="margin-top: 16px;">
        <div class="progress-bar">
          <div class="progress-bar-fill" style="width: {$progressPercent}%;"></div>
        </div>
        <div class="progress-label">{$progressStage} ({$progressPercent}%)</div>
      </div>
    {/if}
  {:else}
    <p class="text-muted">Loading models...</p>
  {/if}
</div>

<!-- Preferences -->
<div class="settings-section">
  <h3>Preferences</h3>

  <div class="settings-row">
    <div>
      <div class="setting-label">Default Note Format</div>
      <div class="setting-desc">Used when creating new sessions</div>
    </div>
    <select bind:value={defaultFormat} style="width: 180px;">
      <option value="soap">SOAP</option>
      <option value="cbt">CBT</option>
      <option value="intake">Intake</option>
    </select>
  </div>

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
    <div class="toggle" class:active={$darkMode} onclick={() => darkMode.set(!$darkMode)}>
      <div class="toggle-knob"></div>
    </div>
  </div>

  <div class="settings-row">
    <div>
      <div class="setting-label">Default LLM Model</div>
      <div class="setting-desc">Used for note generation</div>
    </div>
    <select bind:value={selectedLlm} style="width: 180px;">
      {#if models}
        {#each Object.entries(models.llm) as [name, info]}
          <option value={name}>{info.display}</option>
        {/each}
      {/if}
    </select>
  </div>

  <div class="settings-row">
    <div>
      <div class="setting-label">Default Transcription Model</div>
      <div class="setting-desc">Used for audio transcription</div>
    </div>
    <select bind:value={selectedTranscription} style="width: 180px;">
      {#if models}
        {#each Object.entries(models.transcription) as [name, info]}
          <option value={name}>{info.display}</option>
        {/each}
      {/if}
    </select>
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

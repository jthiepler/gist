<script lang="ts">
  import { onMount } from "svelte";
  import { listModels, downloadModel, getSetting, setSetting } from "$lib/rpc";
  import { sidecarRunning, progressPercent, progressStage } from "$lib/stores";
  import { isRunning, startSidecar, stopSidecar } from "$lib/rpc";
  import type { ModelsResult } from "$lib/types";
  import ProgressBar from "$lib/components/ProgressBar.svelte";
  import Card from "$lib/components/Card.svelte";

  let models = $state<ModelsResult | null>(null);
  let downloading = $state("");
  let defaultFormat = $state("soap");
  let selectedLlm = $state("qwen-3.5-4b");
  let selectedTranscription = $state("whisper-base");
  let error = $state("");
  let saved = $state(false);

  onMount(async () => {
    try {
      const running = await isRunning();
      sidecarRunning.set(running);
    } catch {}

    try {
      models = await listModels();
    } catch {
      // Sidecar not running
    }

    // Load saved settings
    try {
      const fmt = await getSetting("default_format");
      if (fmt) defaultFormat = fmt;
      const llm = await getSetting("default_llm");
      if (llm) selectedLlm = llm;
      const tr = await getSetting("default_transcription");
      if (tr) selectedTranscription = tr;
    } catch {}
  });

  async function handleDownload(model: string, kind: string) {
    downloading = model;
    progressPercent.set(0);
    progressStage.set("Starting download...");

    const ok = await ensureSidecarStart();
    if (!ok) {
      downloading = "";
      return;
    }

    try {
      await downloadModel(model, kind);
      error = "";
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

  async function ensureSidecarStart(): Promise<boolean> {
    try {
      const running = await isRunning();
      if (running) {
        sidecarRunning.set(true);
        return true;
      }
      await startSidecar();
      sidecarRunning.set(true);
      return true;
    } catch (e) {
      error = String(e);
      return false;
    }
  }

  async function handleStart() {
    try {
      await startSidecar();
      sidecarRunning.set(true);
      if (!models) {
        models = await listModels();
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function handleStop() {
    try {
      await stopSidecar();
      sidecarRunning.set(false);
    } catch (e) {
      error = String(e);
    }
  }

  async function saveSettings() {
    try {
      await setSetting("default_format", defaultFormat);
      await setSetting("default_llm", selectedLlm);
      await setSetting("default_transcription", selectedTranscription);
      saved = true;
      setTimeout(() => saved = false, 3000);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="page-header">
  <h2>Settings</h2>
  <p>Configure models and preferences</p>
</div>

{#if error}
  <div class="card" style="border-color: var(--error);">
    <p style="color: var(--error); font-size: 13px;">{error}</p>
  </div>
{/if}

{#if saved}
  <div class="card" style="border-color: var(--success);">
    <p style="color: var(--success); font-size: 13px;">Saved.</p>
  </div>
{/if}

<Card title="Sidecar">
  <div class="toggle-row">
    <span style="font-size: 13px;">Status: {$sidecarRunning ? "Running" : "Stopped"}</span>
    {#if $sidecarRunning}
      <button class="btn btn-danger" onclick={handleStop}>Stop Sidecar</button>
    {:else}
      <button class="btn btn-primary" onclick={handleStart}>Start Sidecar</button>
    {/if}
  </div>
</Card>

{#if models}
  <Card title="LLM Models">
    <table style="width: 100%; border-collapse: collapse; font-size: 14px;">
      <thead>
        <tr style="border-bottom: 1px solid var(--border); text-align: left;">
          <th style="padding: 8px 12px;">Name</th>
          <th style="padding: 8px 12px;">Backend</th>
          <th style="padding: 8px 12px;">Size</th>
          <th style="padding: 8px 12px;">Action</th>
        </tr>
      </thead>
      <tbody>
        {#each Object.entries(models.llm) as [name, info]}
          <tr style="border-bottom: 1px solid var(--border);">
            <td style="padding: 8px 12px;">{info.display}</td>
            <td style="padding: 8px 12px;">{info.backend}</td>
            <td style="padding: 8px 12px;">{info.size_gb} GB</td>
            <td style="padding: 8px 12px;">
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
  </Card>

  <Card title="Transcription Models">
    <table style="width: 100%; border-collapse: collapse; font-size: 14px;">
      <thead>
        <tr style="border-bottom: 1px solid var(--border); text-align: left;">
          <th style="padding: 8px 12px;">Name</th>
          <th style="padding: 8px 12px;">Backend</th>
          <th style="padding: 8px 12px;">Size</th>
          <th style="padding: 8px 12px;">Action</th>
        </tr>
      </thead>
      <tbody>
        {#each Object.entries(models.transcription) as [name, info]}
          <tr style="border-bottom: 1px solid var(--border);">
            <td style="padding: 8px 12px;">{info.display}</td>
            <td style="padding: 8px 12px;">{info.backend}</td>
            <td style="padding: 8px 12px;">{info.size_gb} GB</td>
            <td style="padding: 8px 12px;">
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
  </Card>

  <ProgressBar visible={!!downloading} />
{/if}

<Card title="Preferences">
  <div class="form-group">
    <label for="def-format">Default Note Format</label>
    <select id="def-format" bind:value={defaultFormat}>
      <option value="soap">SOAP Note</option>
      <option value="cbt">CBT Note</option>
      <option value="intake">Intake Note</option>
    </select>
  </div>
  <div class="form-group">
    <label for="def-llm">Default LLM Model</label>
    <select id="def-llm" bind:value={selectedLlm}>
      {#if models}
        {#each Object.entries(models.llm) as [name, info]}
          <option value={name}>{info.display}</option>
        {/each}
      {/if}
    </select>
  </div>
  <div class="form-group">
    <label for="def-tr">Default Transcription Model</label>
    <select id="def-tr" bind:value={selectedTranscription}>
      {#if models}
        {#each Object.entries(models.transcription) as [name, info]}
          <option value={name}>{info.display}</option>
        {/each}
      {/if}
    </select>
  </div>
  <button class="btn btn-primary" onclick={saveSettings}>Save Preferences</button>
</Card>

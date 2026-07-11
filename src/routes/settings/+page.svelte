<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    deleteModel,
    downloadModel,
    listModels,
  } from "$lib/rpc";
  import {
    activeOperation,
    appearance,
    progressPercent,
    progressStage,
    sidecarBusy,
  } from "$lib/stores";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import { loadSettings, saveSetting } from "$lib/settings";
  import type { ModelsResult } from "$lib/types";

  let models = $state<ModelsResult | null>(null);
  let downloading = $state("");
  let deleting = $state("");
  let thinking = $state(false);
  let selectedLlm = $state("");
  let error = $state("");
  let saveState = $state<"idle" | "saving" | "saved">("idle");
  let pendingSaves = 0;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let saveQueue: Promise<void> = Promise.resolve();
  let confirmRecordingConsent = $state(true);

  onDestroy(() => {
    if (saveTimer) clearTimeout(saveTimer);
  });

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
    } catch (e) {
      console.error("Failed to load models:", e);
      error = "Failed to load models.";
    }

    const s = await loadSettings();
    if (s.defaultLlm) selectedLlm = s.defaultLlm;
    thinking = s.thinking;
    confirmRecordingConsent = s.confirmRecordingConsent;
    appearance.set(s.appearance);
  });

  async function handleDownload(model: string) {
    if ($sidecarBusy) {
      error = "Another operation is in progress. Please wait or cancel it first.";
      return;
    }
    downloading = model;
    progressPercent.set(0);
    progressStage.set("Downloading model...");
    activeOperation.set({ type: "download_model", label: "Downloading model..." });
    try {
      await downloadModel(model);
      await refreshModels();
    } catch (e) {
      const msg = String(e);
      error =
        msg === "sidecar_busy"
          ? "Another operation is in progress. Please wait or cancel it first."
          : `Download failed: ${msg}`;
    } finally {
      downloading = "";
      progressPercent.set(0);
      progressStage.set("");
      activeOperation.set({ type: null, label: "" });
    }
  }

  async function handleDelete(model: string) {
    if ($sidecarBusy) {
      error = "Another operation is in progress. Please wait or cancel it first.";
      return;
    }
    deleting = model;
    try {
      await deleteModel(model);
      await refreshModels();
      if (selectedLlm === model && models && Object.keys(models.llm).length > 0) {
        selectedLlm = Object.keys(models.llm)[0];
      }
    } catch (e) {
      const msg = String(e);
      error =
        msg === "sidecar_busy"
          ? "Another operation is in progress. Please wait or cancel it first."
          : `Delete failed: ${msg}`;
    } finally {
      deleting = "";
    }
  }

  async function persistSetting(key: string, value: string) {
    pendingSaves += 1;
    saveState = "saving";
    if (saveTimer) clearTimeout(saveTimer);
    error = "";

    const save = saveQueue.then(() => saveSetting(key, value));
    saveQueue = save.catch(() => {});
    let succeeded = false;

    try {
      await save;
      succeeded = true;
    } catch (e) {
      error = `Could not save setting: ${String(e)}`;
    } finally {
      pendingSaves -= 1;
      if (pendingSaves > 0) {
        saveState = "saving";
      } else if (succeeded) {
        saveState = "saved";
        saveTimer = setTimeout(() => (saveState = "idle"), 2400);
      } else {
        saveState = "idle";
      }
    }
  }

  async function selectModel(name: string) {
    selectedLlm = name;
    await persistSetting("default_llm", name);
  }

  async function toggleThinking() {
    thinking = !thinking;
    await persistSetting("thinking", String(thinking));
  }

  async function toggleRecordingConsent() {
    confirmRecordingConsent = !confirmRecordingConsent;
    await persistSetting("confirm_recording_consent", String(confirmRecordingConsent));
  }

  async function setAppearance(value: "system" | "light" | "dark") {
    appearance.set(value);
    await persistSetting("appearance", value);
  }

  function modelRamRecommendation(name: string) {
    if (name.includes("9b")) return "Recommended for 16GB+ RAM";
    if (name.includes("4b")) return "Recommended for 8GB+ RAM";
    return "";
  }
</script>

<div class="workspace-header">
  <h2>Settings</h2>
  <div class="header-meta">Manage AI models, appearance, and local privacy settings.</div>
  <div class="settings-save-status" aria-live="polite">
    {#if saveState === "saving"}Saving changes…{:else if saveState === "saved"}Changes saved automatically{/if}
  </div>
</div>

{#if error}
  <div class="error-banner">{error}</div>
{/if}

<div class="settings-section">
  <h3>Note generation</h3>

  <div class="model-group">
    <div class="model-group-title">Note-writing model</div>
    <p class="text-muted settings-help">Models are downloaded once and run on this device for note generation.</p>

    {#if models}
      <table class="model-table">
        <thead>
          <tr>
            <th style="width: 30%;">Name</th>
            <th>Description</th>
            <th style="width: 70px;">Size</th>
            <th style="width: 110px;"></th>
          </tr>
        </thead>
        <tbody>
          {#each Object.entries(models.llm) as [name, info]}
            <tr
              class="model-row {info.downloaded ? 'model-available' : 'model-not-downloaded'}"
              class:model-selected={selectedLlm === name}
              onclick={() => info.downloaded && selectModel(name)}
              onkeydown={(event) => {
                if (info.downloaded && (event.key === "Enter" || event.key === " ")) {
                  event.preventDefault();
                  void selectModel(name);
                }
              }}
              tabindex={info.downloaded ? 0 : undefined}
              role={info.downloaded ? "button" : undefined}
            >
              <td>
                <div class="model-name">{info.display}</div>
                <div class="model-lifecycle">{selectedLlm === name ? "Active" : info.downloaded ? "Installed" : downloading === name ? "Downloading" : "Not installed"}</div>
              </td>
              <td class="model-desc">
                <div>{info.description}</div>
                <div class="model-ram">{modelRamRecommendation(name)}</div>
              </td>
              <td>{info.size_gb} GB</td>
              <td>
                {#if info.downloaded}
                  {#if selectedLlm === name}
                    <span class="model-selected-marker">Selected</span>
                  {:else}
                    <button
                      class="btn btn-sm btn-danger"
                      onclick={(e) => { e.stopPropagation(); handleDelete(name); }}
                      disabled={deleting === name}
                    >
                      {deleting === name ? "Removing…" : "Remove"}
                    </button>
                  {/if}
                {:else}
                  <button
                    class="btn btn-sm btn-primary"
                    onclick={(e) => { e.stopPropagation(); handleDownload(name); }}
                    disabled={downloading === name}
                  >
                    {downloading === name ? "Downloading…" : "Download"}
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <p class="text-muted">Loading models...</p>
    {/if}
  </div>

  <div class="settings-row">
    <div>
      <div class="setting-label">Detailed reasoning</div>
      <div class="setting-desc">Give the model more time before writing notes.</div>
    </div>
    <button
      type="button"
      class="toggle"
      class:active={thinking}
      role="switch"
      aria-checked={thinking}
      aria-label="Detailed reasoning"
      onclick={toggleThinking}
    >
      <div class="toggle-knob"></div>
    </button>
  </div>

</div>

<div class="settings-section">
  <h3>Appearance</h3>
  <div class="settings-row">
    <div>
      <div class="setting-label">Appearance</div>
      <div class="setting-desc">Choose whether Gist follows the system theme or uses a fixed appearance.</div>
    </div>
    <select class="appearance-select" value={$appearance} onchange={(event) => setAppearance((event.currentTarget as HTMLSelectElement).value as "system" | "light" | "dark")} aria-label="Appearance">
      <option value="system">System</option>
      <option value="light">Light</option>
      <option value="dark">Dark</option>
    </select>
  </div>

</div>

<div class="settings-section local-privacy-panel">
  <h3>Local processing and storage</h3>
  <p class="settings-help">Session audio, transcripts, and generated notes are processed and stored on this device. No session content is sent to a remote AI service.</p>
  <p class="settings-help">An internet connection may be required to download local model files or application updates.</p>
  <div class="settings-row">
    <div>
      <div class="setting-label">Recording consent confirmation</div>
      <div class="setting-desc">Ask for confirmation before starting a new session recording.</div>
    </div>
    <button
      type="button"
      class="toggle"
      class:active={confirmRecordingConsent}
      role="switch"
      aria-checked={confirmRecordingConsent}
      aria-label="Recording consent confirmation"
      onclick={toggleRecordingConsent}
    >
      <div class="toggle-knob"></div>
    </button>
  </div>
</div>

<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    deleteModel,
    downloadModel,
    exportBackup,
    exportHumanArchive,
    getSystemMemoryBytes,
    listModels,
    pickBackupForRestore,
    restoreBackup,
    setMenuBarEnabled,
  } from "$lib/rpc";
  import {
    activeOperation,
    appearance,
    progressPercent,
    progressStage,
    sidecarBusy,
  } from "$lib/stores";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import {
    AVAILABLE_LLM_MODELS,
    createModelState,
    DEFAULT_LLM,
    EVIDENCE_LLM,
    mergeDownloadedState,
    recommendedLlmForMemory,
  } from "$lib/models";
  import { loadSettings, saveSetting } from "$lib/settings";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { confirm, message } from "@tauri-apps/plugin-dialog";
  import { relaunch } from "@tauri-apps/plugin-process";
  import {
    FEEDBACK_EMAIL_URL,
    FEEDBACK_GITHUB_URL,
    FEEDBACK_SURVEY_URL,
    dismissFeedbackPrompt,
  } from "$lib/feedback";
  import type { ModelsResult, RecordCounts } from "$lib/types";

  type DataOperation = "backup" | "archive" | "restore";

  let models = $state<ModelsResult>(createModelState());
  let modelStatus = $state<"unknown" | "loading" | "ready">("unknown");
  let downloading = $state("");
  let deleting = $state("");
  let selectedLlm = $state(DEFAULT_LLM);
  let menuBarEnabled = $state(true);
  let error = $state("");
  let saveState = $state<"idle" | "saving" | "saved">("idle");
  let pendingSaves = 0;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let saveQueue: Promise<void> = Promise.resolve();
  let developerFeatures = $state(false);
  let captureNoteDiagnostics = $state(false);
  let settingVersions = new Map<string, number>();
  let sidecarAvailable = false;
  let modelRefreshInFlight = false;
  let stopBusySubscription: (() => void) | undefined;
  let totalMemoryGb = $state<number | null>(null);
  let dataOperation = $state<"" | DataOperation>("");
  let dataPassphrase = $state("");
  let validExportPassphrase = $derived(
    dataPassphrase.trim().length === 0 || Array.from(dataPassphrase.trim()).length >= 12,
  );
  let recommendedLlm = $derived(
    totalMemoryGb === null ? null : recommendedLlmForMemory(totalMemoryGb),
  );

  onDestroy(() => {
    if (saveTimer) clearTimeout(saveTimer);
    sidecarAvailable = false;
    stopBusySubscription?.();
  });

  async function refreshModels() {
    if (!sidecarAvailable || $sidecarBusy || modelRefreshInFlight) return;
    modelRefreshInFlight = true;
    modelStatus = "loading";
    try {
      const result = await listModels();
      models = mergeDownloadedState(result);
      modelStatus = "ready";
    } catch (e) {
      // The catalog is local and remains usable when the sidecar is busy or
      // unavailable. A later busy-state transition will retry this refresh.
      console.warn("Could not refresh model availability:", e);
      modelStatus = "unknown";
    } finally {
      modelRefreshInFlight = false;
    }
  }

  onMount(async () => {
    // Settings are local Tauri state and should not wait for Python.
    const [s, ok] = await Promise.all([
      loadSettings(),
      ensureSidecar(),
      getSystemMemoryBytes()
        .then((bytes) => {
          totalMemoryGb = bytes / (1024 ** 3);
        })
        .catch((e) => {
          console.warn("Could not detect system memory:", e);
        }),
    ]);
    if (s.defaultLlm && AVAILABLE_LLM_MODELS.some((model) => model.name === s.defaultLlm)) {
      selectedLlm = s.defaultLlm;
    }
    menuBarEnabled = s.menuBarEnabled;
    developerFeatures = s.developerFeaturesEnabled;
    captureNoteDiagnostics = s.captureNoteDiagnostics;
    appearance.set(s.appearance);

    sidecarAvailable = ok;
    if (!ok) {
      error = "Failed to start the processing engine. Model availability will be checked when it is running.";
      return;
    }

    // Retry after any operation that was already in progress finishes.
    stopBusySubscription = sidecarBusy.subscribe((busy) => {
      if (!busy) void refreshModels();
    });
    void refreshModels();
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
    if (model === EVIDENCE_LLM) {
      error = "Qwen 3.5 4B is required for evidence extraction and cannot be removed in Gist.";
      return;
    }
    if ($sidecarBusy) {
      error = "Another operation is in progress. Please wait or cancel it first.";
      return;
    }
    deleting = model;
    try {
      await deleteModel(model);
      await refreshModels();
      if (selectedLlm === model) {
        selectedLlm =
          AVAILABLE_LLM_MODELS.find((candidate) => models.llm[candidate.name]?.downloaded === true)?.name ??
          DEFAULT_LLM;
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

  async function persistSetting(
    key: string,
    value: string,
    write: () => Promise<void> = () => saveSetting(key, value),
  ): Promise<boolean> {
    pendingSaves += 1;
    saveState = "saving";
    if (saveTimer) clearTimeout(saveTimer);
    error = "";

    const save = saveQueue.then(write);
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
    return succeeded;
  }

  async function selectModel(name: string) {
    const previous = selectedLlm;
    const version = (settingVersions.get("default_llm") ?? 0) + 1;
    settingVersions.set("default_llm", version);
    selectedLlm = name;
    if (!(await persistSetting("default_llm", name)) && settingVersions.get("default_llm") === version) {
      selectedLlm = previous;
    }
  }

  async function toggleNoteDiagnostics() {
    if (!developerFeatures) return;
    const previous = captureNoteDiagnostics;
    const version = (settingVersions.get("capture_note_generation_diagnostics") ?? 0) + 1;
    settingVersions.set("capture_note_generation_diagnostics", version);
    captureNoteDiagnostics = !captureNoteDiagnostics;
    if (
      !(await persistSetting(
        "capture_note_generation_diagnostics",
        String(captureNoteDiagnostics),
      )) && settingVersions.get("capture_note_generation_diagnostics") === version
    ) {
      captureNoteDiagnostics = previous;
    }
  }

  async function toggleMenuBar() {
    const previous = menuBarEnabled;
    const next = !previous;
    const version = (settingVersions.get("menu_bar_enabled") ?? 0) + 1;
    settingVersions.set("menu_bar_enabled", version);
    menuBarEnabled = next;
    if (
      !(await persistSetting(
        "menu_bar_enabled",
        String(next),
        () => setMenuBarEnabled(next),
      )) && settingVersions.get("menu_bar_enabled") === version
    ) {
      menuBarEnabled = previous;
    }
  }

  async function setAppearance(value: "system" | "light" | "dark") {
    const previous = $appearance;
    const version = (settingVersions.get("appearance") ?? 0) + 1;
    settingVersions.set("appearance", version);
    appearance.set(value);
    if (!(await persistSetting("appearance", value)) && settingVersions.get("appearance") === version) {
      appearance.set(previous);
    }
  }

  function isRecommendedModel(name: string) {
    return name === recommendedLlm;
  }

  function modelPresentation(name: string, info: ModelsResult["llm"][string]) {
    return AVAILABLE_LLM_MODELS.find((model) => model.name === name) ?? {
      title: info.display,
      caption: info.description,
      description: info.description,
    };
  }

  function modelLifecycle(name: string, info: ModelsResult["llm"][string]) {
    if (downloading === name) return "Downloading";
    if (selectedLlm === name) return "Selected";
    if (info.downloaded === null) return $sidecarBusy ? "Checking when processing finishes" : "Checking availability";
    if (name === EVIDENCE_LLM && info.downloaded) return "Required for evidence extraction";
    return info.downloaded ? "Installed" : "Not installed";
  }

  async function openFeedbackLink(url: string) {
    try {
      await openUrl(url);
      await dismissFeedbackPrompt();
    } catch (e) {
      error = `Could not open feedback link: ${String(e)}`;
    }
  }

  function currentDataPassphrase(): string | null {
    return dataPassphrase.trim().length === 0 ? null : dataPassphrase;
  }

  function recordCountSummary(result: RecordCounts): string {
    return `${result.patient_count} patient${result.patient_count === 1 ? "" : "s"} and ${result.session_count} session${result.session_count === 1 ? "" : "s"}`;
  }

  async function runDataOperation(
    operation: DataOperation,
    failureMessage: string,
    action: (passphrase: string | null) => Promise<void>,
  ) {
    if (dataOperation) return;
    dataOperation = operation;
    error = "";
    try {
      await action(currentDataPassphrase());
    } catch (e) {
      error = `${failureMessage}: ${String(e)}`;
    } finally {
      dataPassphrase = "";
      dataOperation = "";
    }
  }

  async function handleBackupExport() {
    await runDataOperation("backup", "Could not create backup", async (passphrase) => {
      const result = await exportBackup(passphrase);
      if (result) {
        await message(
          `Backed up ${recordCountSummary(result)}.\n\n${result.path}`,
          { title: "Backup created", kind: "info" },
        );
      }
    });
  }

  async function handleArchiveExport() {
    await runDataOperation("archive", "Could not create record archive", async (passphrase) => {
      const result = await exportHumanArchive(passphrase);
      if (result) {
        await message(
          `Exported a readable archive containing ${recordCountSummary(result)}.\n\n${result.path}`,
          { title: "Record archive created", kind: "info" },
        );
      }
    });
  }

  async function handleRestore() {
    await runDataOperation("restore", "Could not restore backup", async (passphrase) => {
      const backup = await pickBackupForRestore(passphrase);
      if (!backup) return;
      const approved = await confirm(
        `This backup contains ${recordCountSummary(backup)}.\n\nRestoring replaces the entire current Gist library. Gist will keep an automatic rollback copy of the current library.`,
        {
          title: "Replace current library?",
          kind: "warning",
          okLabel: "Restore and replace",
          cancelLabel: "Cancel",
        },
      );
      if (!approved) return;
      const result = await restoreBackup(backup.path, passphrase);
      await message(
        `Restored ${recordCountSummary(result)}. Gist will now restart.`,
        { title: "Backup restored", kind: "info" },
      );
      await relaunch();
    });
  }
</script>

<div class="workspace-header">
  <h2>Settings</h2>
  <div class="settings-save-status" aria-live="polite">
    {#if saveState === "saving"}Saving changes…{:else if saveState === "saved"}Changes saved automatically{/if}
  </div>
</div>

{#if error}
  <div class="error-banner" role="alert">{error}</div>
{/if}

<div class="settings-section">
  <h3>Note generation</h3>

  <div class="model-group">
    <div class="model-group-title">Note-writing model</div>
    <p class="text-muted settings-help">
      Models are downloaded once and run on this device.
    </p>

    <table class="model-table">
      <thead>
        <tr>
          <th style="width: 30%;">Name</th>
          <th>What to expect</th>
          <th style="width: 70px;">Size</th>
          <th style="width: 110px;"></th>
        </tr>
      </thead>
      <tbody>
        {#each AVAILABLE_LLM_MODELS as model}
          {@const name = model.name}
          {@const info = models.llm[name]}
          {#if info}
          {@const presentation = modelPresentation(name, info)}
          <tr
            class="model-row {info.downloaded === true ? 'model-available' : 'model-not-downloaded'}"
            class:model-selected={selectedLlm === name}
            onclick={() => info.downloaded === true && selectModel(name)}
            onkeydown={(event) => {
              if (info.downloaded === true && (event.key === "Enter" || event.key === " ")) {
                event.preventDefault();
                void selectModel(name);
              }
            }}
            tabindex={info.downloaded === true ? 0 : undefined}
            role={info.downloaded === true ? "button" : undefined}
          >
            <td>
              <div class="model-name">{presentation.title}</div>
              <div class="model-caption">
                {presentation.caption}{#if isRecommendedModel(name)} · Recommended for this device{/if}
              </div>
              <div class="model-lifecycle">{modelLifecycle(name, info)}</div>
            </td>
            <td class="model-desc">
              <div>{presentation.description}</div>
            </td>
            <td>{info.size_gb} GB</td>
            <td>
              {#if info.downloaded === true}
                {#if selectedLlm === name}
                  <span class="model-selected-marker">Selected</span>
                {:else if name === EVIDENCE_LLM}
                  <span class="model-selected-marker">Required</span>
                {:else}
                  <button
                    class="btn btn-sm btn-danger"
                    onclick={(e) => { e.stopPropagation(); handleDelete(name); }}
                    disabled={$sidecarBusy || downloading !== "" || deleting !== ""}
                  >
                    {deleting === name ? "Removing…" : "Remove"}
                  </button>
                {/if}
              {:else if info.downloaded === false}
                <button
                  class="btn btn-sm btn-primary"
                  onclick={(e) => { e.stopPropagation(); handleDownload(name); }}
                  disabled={$sidecarBusy || downloading !== "" || deleting !== ""}
                >
                  {downloading === name ? "Downloading…" : "Download"}
                </button>
              {:else}
                <button class="btn btn-sm" disabled>
                  {modelStatus === "loading" ? "Checking…" : "Unavailable"}
                </button>
              {/if}
            </td>
          </tr>
          {/if}
        {/each}
      </tbody>
    </table>
    {#if modelStatus === "unknown" && $sidecarBusy}
      <p class="text-muted settings-help">Download status will update when the current processing operation finishes.</p>
    {/if}
  </div>

</div>

{#if developerFeatures}
  <div class="settings-section">
    <h3>Developer diagnostics</h3>
    <p class="settings-help">
      Local developer builds only. Captured runs contain complete clinical source material, model prompts,
      intermediate evidence, and generated notes. They remain on this Mac until the session is deleted.
    </p>
    <div class="settings-row">
      <div>
        <div class="setting-label">Capture note-generation pipeline</div>
        <div class="setting-desc">Save every stage of future note generations so it can be exported from the session menu.</div>
      </div>
      <button
        type="button"
        class="toggle"
        class:active={captureNoteDiagnostics}
        role="switch"
        aria-checked={captureNoteDiagnostics}
        aria-label="Capture note-generation pipeline"
        onclick={toggleNoteDiagnostics}
      >
        <div class="toggle-knob"></div>
      </button>
    </div>
  </div>
{/if}

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
  <div class="settings-row">
    <div>
      <div class="setting-label">Show Gist in the menu bar</div>
      <div class="setting-desc">Start, pause, and stop recordings without opening the main window.</div>
    </div>
    <button
      type="button"
      class="toggle"
      class:active={menuBarEnabled}
      role="switch"
      aria-checked={menuBarEnabled}
      aria-label="Show Gist in the menu bar"
      onclick={toggleMenuBar}
    >
      <div class="toggle-knob"></div>
    </button>
  </div>
</div>

<div class="settings-section">
  <h3>Data and backups</h3>
  <div class="settings-row data-passphrase-row">
    <div>
      <div class="setting-label">Export or restore passphrase</div>
      <div class="setting-desc">Optional. Use at least 12 characters to encrypt a new export. Gist cannot recover a forgotten passphrase.</div>
    </div>
    <input
      class="data-passphrase-input"
      type="password"
      bind:value={dataPassphrase}
      autocomplete="new-password"
      placeholder="Optional passphrase"
      aria-label="Export or restore passphrase"
      disabled={dataOperation !== ""}
    />
  </div>
  {#if !validExportPassphrase}
    <p class="error-text">New encrypted exports require at least 12 characters. Shorter passphrases can only be used to restore an existing backup.</p>
  {/if}
  <div class="settings-row">
    <div>
      <div class="setting-label">Restorable Gist backup</div>
      <div class="setting-desc">Create a verified snapshot of patients, sessions, sources, notes, revisions, and custom templates.</div>
    </div>
    <button class="btn btn-primary" type="button" onclick={handleBackupExport} disabled={dataOperation !== "" || $sidecarBusy || !validExportPassphrase}>
      {dataOperation === "backup" ? "Creating…" : "Create backup"}
    </button>
  </div>
  <div class="settings-row">
    <div>
      <div class="setting-label">Human-readable record archive</div>
      <div class="setting-desc">Export an easy-to-browse ZIP of plainly named folders and text files. Leave the passphrase blank for a normal ZIP that opens without special software.</div>
    </div>
    <button class="btn" type="button" onclick={handleArchiveExport} disabled={dataOperation !== "" || $sidecarBusy || !validExportPassphrase}>
      {dataOperation === "archive" ? "Exporting…" : "Export archive"}
    </button>
  </div>
  <div class="settings-row">
    <div>
      <div class="setting-label">Restore a Gist backup</div>
      <div class="setting-desc">Validate a backup, preserve the current library as a rollback copy, and replace all current records.</div>
    </div>
    <button class="btn btn-danger" type="button" onclick={handleRestore} disabled={dataOperation !== "" || $sidecarBusy}>
      {dataOperation === "restore" ? "Restoring…" : "Restore backup"}
    </button>
  </div>
</div>

<div class="settings-section">
  <h3>Feedback</h3>
  <p class="settings-help">Help shape Gist with general feedback. Please do not include real patient information.</p>
  <div class="feedback-settings-links">
    <button class="btn btn-primary" type="button" onclick={() => openFeedbackLink(FEEDBACK_SURVEY_URL)}>Take the short survey</button>
    <button class="btn" type="button" onclick={() => openFeedbackLink(FEEDBACK_EMAIL_URL)}>Email feedback</button>
    <button class="btn" type="button" onclick={() => openFeedbackLink(FEEDBACK_GITHUB_URL)}>Open GitHub Issues</button>
  </div>
</div>

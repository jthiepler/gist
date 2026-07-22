<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { ensureSidecar } from "$lib/ensureSidecar";
  import {
    AVAILABLE_LLM_MODELS,
    DEFAULT_LLM,
    createModelState,
    mergeDownloadedState,
    missingRequiredLlmModels,
    recommendedLlmForMemory,
    EVIDENCE_LLM,
  } from "$lib/models";
  import {
    cancelSidecar,
    downloadModel,
    getSetting,
    getSystemMemoryBytes,
    listModels,
    setSetting,
  } from "$lib/rpc";
  import { activeOperation, progressPercent, progressStage, sidecarBusy } from "$lib/stores";
  import type { ModelsResult } from "$lib/types";

  let { onComplete }: { onComplete: () => void } = $props();

  let step = $state<0 | 1 | 2 | 3>(0);
  let models = $state<ModelsResult>(createModelState());
  let selectedModel = $state(DEFAULT_LLM);
  let downloading = $state("");
  let error = $state("");
  let cancelling = $state(false);
  let totalMemoryGb = $state<number | null>(null);
  let returningUser = $state(false);
  let checkingModels = $state(true);
  let retryWhenIdle = false;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;
  let stopBusySubscription: (() => void) | undefined;

  let selectedPresentation = $derived(
    AVAILABLE_LLM_MODELS.find((model) => model.name === selectedModel) ?? AVAILABLE_LLM_MODELS[0],
  );
  let selectedNoteInstalled = $derived(models.llm[selectedModel]?.downloaded === true);
  let evidenceModelInstalled = $derived(
    models.llm[EVIDENCE_LLM]?.downloaded === true,
  );
  let selectedInstalled = $derived(selectedNoteInstalled && evidenceModelInstalled);
  let missingRequiredModels = $derived(missingRequiredLlmModels(selectedModel, models));
  let requiredDownloadSize = $derived(
    missingRequiredModels.reduce(
      (total, name) =>
        total + (AVAILABLE_LLM_MODELS.find((model) => model.name === name)?.sizeGb ?? 0),
      0,
    ),
  );
  let installedModel = $derived(
    AVAILABLE_LLM_MODELS.find((model) => models.llm[model.name]?.downloaded === true)?.name ?? null,
  );
  let missingSelectedModel = $derived(!selectedNoteInstalled);
  let missingEvidenceModel = $derived(!evidenceModelInstalled);

  onDestroy(() => {
    if (retryTimer) clearTimeout(retryTimer);
    stopBusySubscription?.();
  });

  onMount(() => {
    stopBusySubscription = sidecarBusy.subscribe((busy) => {
      if (!busy && retryWhenIdle) {
        retryWhenIdle = false;
        void initialize();
      }
    });
    void initialize();
  });

  function scheduleBusyRetry() {
    retryWhenIdle = true;
    if (retryTimer) clearTimeout(retryTimer);
    retryTimer = setTimeout(() => {
      retryTimer = null;
      if (!$sidecarBusy && retryWhenIdle) {
        retryWhenIdle = false;
        void initialize();
      }
    }, 500);
  }

  async function initialize() {
    if (checkingModels && step !== 0) return;
    retryWhenIdle = false;
    if (retryTimer) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
    checkingModels = true;
    error = "";
    step = 0;

    let completed: string | null;
    let savedModel: string | null;
    let memory: number | null;
    try {
      [completed, savedModel, memory] = await Promise.all([
        getSetting("onboarding_completed"),
        getSetting("default_llm"),
        getSystemMemoryBytes().catch(() => null),
      ]);
    } catch (e) {
      checkingModels = false;
      error = `Gist could not read its setup settings: ${String(e)}`;
      return;
    }

    returningUser = completed === "true";

    if (memory !== null) totalMemoryGb = memory / (1024 ** 3);
    const recommendation = recommendedLlmForMemory(totalMemoryGb ?? 0);
    selectedModel =
      savedModel && AVAILABLE_LLM_MODELS.some((model) => model.name === savedModel)
        ? savedModel
        : recommendation;

    if (!(await ensureSidecar())) {
      checkingModels = false;
      error = "Gist could not start its local processing engine. Try again to continue setup.";
      return;
    }

    try {
      models = mergeDownloadedState(await listModels());
    } catch (e) {
      const message = String(e);
      if (message === "sidecar_busy" || message.toLowerCase().includes("busy")) {
        error = "Gist is finishing another local operation. Model availability will be checked again when it is ready.";
        scheduleBusyRetry();
      } else {
        error = `Gist could not check the installed models: ${message}`;
      }
      checkingModels = false;
      return;
    }

    // A first-time setup can start from an existing download. Returning users
    // keep their saved note-writing choice while a missing dependency is repaired.
    if (!returningUser && models.llm[selectedModel]?.downloaded !== true && installedModel) {
      selectedModel = installedModel;
    }

    checkingModels = false;
    if (returningUser && selectedNoteInstalled && evidenceModelInstalled) {
      onComplete();
      return;
    }

    step = returningUser ? 3 : 1;
  }

  function chooseModel(name: string) {
    if (downloading) return;
    selectedModel = name;
    error = "";
  }

  async function refreshModels() {
    models = mergeDownloadedState(await listModels());
  }

  async function handleDownload() {
    if ($sidecarBusy || downloading) return;
    error = "";
    progressPercent.set(0);

    try {
      const requiredModels = missingRequiredLlmModels(selectedModel, models);
      for (const model of requiredModels) {
        const title =
          AVAILABLE_LLM_MODELS.find((candidate) => candidate.name === model)?.title ?? model;
        downloading = model;
        progressStage.set(`Preparing ${title.toLowerCase()} download...`);
        activeOperation.set({ type: "download_model", label: `Downloading ${title} model…` });
        await downloadModel(model);
        await refreshModels();
        if (models.llm[model]?.downloaded !== true) {
          throw new Error(`The ${title} download finished, but could not be verified.`);
        }
      }
      await setSetting("default_llm", selectedModel);
      if (returningUser && selectedInstalled) {
        await finish();
      }
    } catch (e) {
      const message = String(e);
      if (!cancelling && !message.toLowerCase().includes("cancel")) {
        error = `Download failed: ${message}`;
      }
    } finally {
      downloading = "";
      cancelling = false;
      progressPercent.set(0);
      progressStage.set("");
      activeOperation.set({ type: null, label: "" });
    }
  }

  async function handleCancel() {
    cancelling = true;
    try {
      await cancelSidecar();
    } catch (e) {
      cancelling = false;
      error = `Could not cancel the download: ${String(e)}`;
    }
  }

  async function finish() {
    if (!selectedInstalled) return;
    error = "";
    try {
      await Promise.all([
        setSetting("default_llm", selectedModel),
        setSetting("onboarding_completed", "true"),
      ]);
      onComplete();
    } catch (e) {
      error = `Gist could not save the setup: ${String(e)}`;
    }
  }
</script>

<div class="onboarding-shell" role="dialog" aria-modal="true" aria-label="Set up Gist">
  {#if step === 0}
    <div class="onboarding-loading" role="status">
      <div class="onboarding-mark" aria-hidden="true">G</div>
      {#if checkingModels}
        <span>Checking local models…</span>
      {:else}
        <strong>Gist could not verify its local models</strong>
        {#if error}<p class="onboarding-check-error">{error}</p>{/if}
        <button class="btn btn-primary" onclick={initialize} disabled={$sidecarBusy}>Try again</button>
      {/if}
    </div>
  {:else}
    <div class="onboarding-card onboarding-step-{step}">
      {#if !returningUser}
        <div class="onboarding-progress" aria-label="Setup progress">
          {#each [1, 2, 3] as item}
            <span class:active={item <= step}></span>
          {/each}
        </div>
      {/if}

      {#if step === 1}
        <div class="onboarding-hero-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z"></path>
            <path d="m9 12 2 2 4-4"></path>
          </svg>
        </div>
        <div class="onboarding-eyebrow">Welcome to Gist</div>
        <h1>Clinical notes, processed on your Mac</h1>
        <p class="onboarding-lead">Record or import a session, create a transcript, and generate an editable clinical note—all without sending the conversation to the cloud.</p>

        <div class="onboarding-assurances">
          <div><span aria-hidden="true">✓</span><strong>No account or subscription</strong></div>
          <div><span aria-hidden="true">✓</span><strong>No cloud processing or telemetry</strong></div>
          <div><span aria-hidden="true">✓</span><strong>Client data stays on this Mac</strong></div>
        </div>

        <div class="onboarding-actions">
          <button class="btn btn-primary onboarding-primary" onclick={() => (step = 2)}>Continue</button>
        </div>
      {:else if step === 2}
        <div class="onboarding-eyebrow">Privacy</div>
        <h1>Your data stays under your control</h1>
        <p class="onboarding-lead">Transcripts, client records, and generated notes are stored and processed locally. Gist deletes its own recordings after transcription, except for short-lived interrupted-recording recovery. Imported audio remains in its original location; Gist neither copies nor deletes it. Gist does not upload clinical data or use it for training.</p>

        <div class="local-flow" aria-label="Session material is processed locally into an editable note">
          <div class="local-flow-node">Session material</div>
          <svg viewBox="0 0 32 16" aria-hidden="true"><path d="M2 8h26m-5-5 5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>
          <div class="local-flow-device">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="4" y="3" width="16" height="13" rx="2"/><path d="M8 21h8m-4-5v5"/></svg>
            <strong>Processing on this Mac</strong>
            <span>Local storage</span>
          </div>
          <svg viewBox="0 0 32 16" aria-hidden="true"><path d="M2 8h26m-5-5 5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>
          <div class="local-flow-node">Editable note</div>
        </div>

        <div class="onboarding-notices">
          <p><strong>One initial download.</strong> Gist needs an internet connection to download a note-writing model. The standard workflow can run offline afterward.</p>
          <p><strong>Your responsibility.</strong> Local processing supports privacy, but does not by itself ensure HIPAA or other regulatory compliance. Device security, access, backups, consent, and retention remain your responsibility.</p>
        </div>

        <div class="onboarding-actions split">
          <button class="btn" onclick={() => (step = 1)}>Back</button>
          <button class="btn btn-primary onboarding-primary" onclick={() => (step = 3)}>I understand — continue</button>
        </div>
      {:else}
        {#if returningUser}
          <div class="onboarding-eyebrow">Local model recovery</div>
          <h1>{missingRequiredModels.length > 1 ? "Restore your local models" : missingEvidenceModel && !missingSelectedModel ? "Restore the evidence model" : "Restore your local model"}</h1>
          <p class="onboarding-lead">
            {#if selectedModel === EVIDENCE_LLM}
              Qwen 3.5 4B handles both evidence extraction and final note writing. Its local files are missing and must be downloaded again.
            {:else if missingEvidenceModel}
              Gist needs Qwen 3.5 4B to organize session evidence before {selectedPresentation.title} writes the final note.
              {missingSelectedModel ? "Both models' local files are missing and must be downloaded again." : "Its local files are missing and must be downloaded again."}
            {:else}
              The local files for your selected {selectedPresentation.caption} note-writing model are missing and must be downloaded again.
            {/if}
          </p>

          <div class="onboarding-notices onboarding-recovery-details">
            {#if selectedModel === EVIDENCE_LLM}
              <p><strong>Evidence extraction and note writing:</strong> Qwen 3.5 4B · {selectedPresentation.sizeGb} GB</p>
            {:else if missingEvidenceModel}
              <p><strong>Evidence extraction:</strong> Qwen 3.5 4B · {AVAILABLE_LLM_MODELS.find((model) => model.name === EVIDENCE_LLM)?.sizeGb ?? 0} GB</p>
            {/if}
            {#if missingSelectedModel && selectedModel !== EVIDENCE_LLM}
              <p><strong>Final note writing:</strong> {selectedPresentation.caption} · {selectedPresentation.sizeGb} GB</p>
            {:else if selectedModel !== EVIDENCE_LLM}
              <p><strong>Your note-writing choice stays unchanged:</strong> {selectedPresentation.caption}</p>
            {/if}
          </div>
        {:else}
          <div class="onboarding-eyebrow">Required model</div>
          <h1>Choose your note-writing model</h1>
          <p class="onboarding-lead">Gist needs a language model to turn transcripts into draft notes. It is downloaded once, then runs entirely on your Mac.</p>

          <div class="onboarding-models">
            {#each AVAILABLE_LLM_MODELS as model}
              {@const isRecommended = model.name === recommendedLlmForMemory(totalMemoryGb ?? 0)}
              {@const isInstalled = models.llm[model.name]?.downloaded === true}
              <button
                type="button"
                class="onboarding-model"
                class:selected={selectedModel === model.name}
                onclick={() => chooseModel(model.name)}
                disabled={downloading !== ""}
              >
                <span class="model-choice-indicator" aria-hidden="true"></span>
                <span class="model-choice-copy">
                  <span class="model-choice-heading">
                    <strong>{model.title}</strong>
                    {#if isRecommended}<span class="model-recommended">Recommended</span>{/if}
                    {#if isInstalled}<span class="model-installed">Installed</span>{/if}
                  </span>
                  <span>{model.caption}</span>
                  <small>{model.description}</small>
                </span>
                <span class="model-choice-size">{model.sizeGb} GB</span>
              </button>
            {/each}
          </div>

          <p class="onboarding-download-note">
            Qwen 3.5 4B is required to organize source material before your selected model writes the note.
            If you select 4B, the same download performs both jobs.
          </p>
        {/if}

        {#if downloading}
          <div class="onboarding-download" role="status" aria-live="polite">
            <div class="onboarding-download-heading">
              <strong>{$progressStage || "Downloading model files…"}</strong>
              <span>{$progressPercent}%</span>
            </div>
            <div class="progress-bar"><div class="progress-bar-fill" style:width={`${$progressPercent}%`}></div></div>
            <button class="onboarding-cancel" onclick={handleCancel} disabled={cancelling}>{cancelling ? "Cancelling…" : "Cancel download"}</button>
          </div>
        {:else if selectedInstalled}
          <div class="onboarding-ready" role="status"><span aria-hidden="true">✓</span><div><strong>Model installed and ready</strong><small>You can use Gist offline.</small></div></div>
        {:else}
          <p class="onboarding-download-note">Downloaded from the model provider. No client or session data is sent during these downloads.</p>
        {/if}

        {#if error}<div class="error-banner onboarding-error" role="alert">{error}</div>{/if}

        <div class="onboarding-actions split" class:complete={selectedInstalled}>
          {#if !returningUser && step === 3 && !installedModel}
            <button class="btn" onclick={() => (step = 2)} disabled={downloading !== ""}>Back</button>
          {/if}
          {#if selectedInstalled}
            <button class="btn btn-primary onboarding-primary" onclick={finish}>{returningUser ? "Continue" : "Start using Gist"}</button>
          {:else}
            <button class="btn btn-primary onboarding-primary" onclick={handleDownload} disabled={downloading !== "" || $sidecarBusy}>
              {returningUser ? "Download and continue" : `Download required ${selectedModel === EVIDENCE_LLM || evidenceModelInstalled || selectedNoteInstalled ? "model" : "models"}`} · {requiredDownloadSize.toFixed(1)} GB
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

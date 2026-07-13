<script lang="ts">
  import { relaunch } from "@tauri-apps/plugin-process";
  import { type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";

  interface Props {
    update: Update;
    isBusy: boolean;
    onDismiss: () => void | Promise<void>;
  }

  let { update, isBusy, onDismiss }: Props = $props();
  let installState = $state<"ready" | "installing" | "error">("ready");
  let downloadedBytes = $state(0);
  let contentLength = $state<number | null>(null);
  let error = $state("");

  let downloadPercent = $derived(
    contentLength && contentLength > 0
      ? Math.min(100, Math.round((downloadedBytes / contentLength) * 100))
      : null,
  );

  function handleDownloadEvent(event: DownloadEvent) {
    if (event.event === "Started") {
      contentLength = event.data.contentLength ?? null;
      downloadedBytes = 0;
    } else if (event.event === "Progress") {
      downloadedBytes += event.data.chunkLength;
    }
  }

  async function installUpdate() {
    if (isBusy) {
      error = "Finish the current recording or processing task before installing the update.";
      return;
    }
    installState = "installing";
    error = "";
    try {
      await update.download(handleDownloadEvent);
      if (isBusy) {
        throw new Error("A recording or processing task started while the update was downloading. Try again when it finishes.");
      }
      await update.install();
      await relaunch();
    } catch (e) {
      console.error("Could not install application update:", e);
      error = String(e).replace(/^Error:\s*/, "");
      installState = "error";
    }
  }
</script>

<section class="update-card" role="status" aria-live="polite">
  <div class="update-card-heading">
    <div>
      <div class="update-card-eyebrow">Application update</div>
      <h2>Gist {update.version} is ready</h2>
    </div>
    {#if installState !== "installing"}
      <button class="update-card-close" type="button" onclick={onDismiss} aria-label="Dismiss update">×</button>
    {/if}
  </div>

  {#if installState === "installing"}
    <p class="update-card-message">Downloading and preparing the update. Gist will restart when it is ready.</p>
    <div class="progress-bar" aria-hidden="true">
      <div class="progress-bar-fill" style:width={downloadPercent === null ? "35%" : `${downloadPercent}%`}></div>
    </div>
    <div class="update-card-progress" aria-live="polite">
      {downloadPercent === null ? "Downloading…" : `${downloadPercent}% downloaded`}
    </div>
  {:else}
    <p class="update-card-message">
      {update.body?.trim() || "A new version of Gist is available with improvements and fixes."}
    </p>
    {#if error}
      <p class="update-card-error" role="alert">Could not install the update: {error}</p>
    {/if}
    {#if isBusy}
      <p class="update-card-warning">Finish the current recording or processing task before installing an update.</p>
    {/if}
    <div class="update-card-actions">
      <button class="btn btn-primary" type="button" onclick={installUpdate} disabled={isBusy}>
        {installState === "error" ? "Try again" : "Install and restart"}
      </button>
      <button class="btn" type="button" onclick={onDismiss}>Later</button>
    </div>
  {/if}
</section>

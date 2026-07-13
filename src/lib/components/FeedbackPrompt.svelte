<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    FEEDBACK_EMAIL_URL,
    FEEDBACK_GITHUB_URL,
    FEEDBACK_SURVEY_URL,
  } from "$lib/feedback";

  let {
    onFeedbackAction,
    onRemindLater,
    onDontAskAgain,
  }: {
    onFeedbackAction: () => void | Promise<void>;
    onRemindLater: () => void | Promise<void>;
    onDontAskAgain: () => void | Promise<void>;
  } = $props();

  async function openFeedback(url: string) {
    try {
      await openUrl(url);
      await onFeedbackAction();
    } catch (error) {
      console.error("Could not open feedback link:", error);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") onRemindLater();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="feedback-prompt-shell" role="presentation">
  <div
    class="feedback-prompt-card"
    role="dialog"
    aria-modal="true"
    aria-labelledby="feedback-prompt-title"
    aria-describedby="feedback-prompt-description"
  >
    <div class="feedback-prompt-eyebrow">Help shape Gist</div>
    <h2 id="feedback-prompt-title">How is Gist working for you?</h2>
    <p id="feedback-prompt-description">
      A few minutes of feedback can help make Gist more useful for therapists. Please do not include real patient information.
    </p>

    <div class="feedback-prompt-actions">
      <button class="btn btn-primary" type="button" onclick={() => openFeedback(FEEDBACK_SURVEY_URL)}>
        Take the short survey
      </button>
      <button class="btn" type="button" onclick={() => openFeedback(FEEDBACK_EMAIL_URL)}>
        Email feedback
      </button>
      <button class="btn" type="button" onclick={() => openFeedback(FEEDBACK_GITHUB_URL)}>
        Open GitHub Issues
      </button>
    </div>

    <div class="feedback-prompt-footer">
      <button type="button" onclick={onRemindLater}>Remind me in 10 launches</button>
      <button type="button" onclick={onDontAskAgain}>Don’t ask again</button>
    </div>
  </div>
</div>

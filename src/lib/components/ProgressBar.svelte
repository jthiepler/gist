<script lang="ts">
  import { progressPercent, progressStage } from "$lib/stores";
  let { visible = true }: { visible?: boolean } = $props();
</script>

{#if visible && $progressStage}
  <div class="card">
    <div class="progress-bar">
      <div class="progress-bar-fill" style="width: {$progressPercent}%;"></div>
    </div>
    <div class="progress-label">{$progressStage} ({$progressPercent}%)</div>
  </div>
{/if}

<script lang="ts">
  import { patients } from "$lib/stores";
</script>

<div class="empty-state">
  {#if $patients.length === 0}
    <div class="empty-title">Welcome to Gist</div>
    <div class="empty-desc">
      Add your first patient from the sidebar to get started.
    </div>
  {:else}
    <div class="empty-title">Select a Patient</div>
    <div class="empty-desc">
      Choose a patient from the sidebar to view sessions and notes.
    </div>
  {/if}
</div>

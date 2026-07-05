<script lang="ts">
  let { icon, text }: { icon: string; text: string } = $props();
</script>

<div class="empty-state">
  <div class="empty-icon">{icon}</div>
  <p>{text}</p>
</div>

<script lang="ts">
  let { title, children }: { title: string; children: any } = $props();
</script>

<div class="card">
  <div class="card-header">{title}</div>
  {@render children()}
</div>

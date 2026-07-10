<script lang="ts">
  let {
    target,
    onHeading,
    onInline,
  }: {
    target: "note" | "input";
    onHeading: (level: 1 | 2 | 3) => void;
    onInline: (prefix: string, suffix: string, placeholder: string) => void;
  } = $props();
</script>

<div class="markdown-toolbar" role="toolbar" aria-label={`${target === "note" ? "Note" : "Source"} formatting`}>
  <button class="toolbar-btn" type="button" title="Heading 1" aria-label="Heading 1" onmousedown={(e) => e.preventDefault()} onclick={() => onHeading(1)}>H1</button>
  <button class="toolbar-btn" type="button" title="Heading 2" aria-label="Heading 2" onmousedown={(e) => e.preventDefault()} onclick={() => onHeading(2)}>H2</button>
  <button class="toolbar-btn" type="button" title="Heading 3" aria-label="Heading 3" onmousedown={(e) => e.preventDefault()} onclick={() => onHeading(3)}>H3</button>
  <button class="toolbar-btn toolbar-btn-strong" type="button" title="Bold" aria-label="Bold" onmousedown={(e) => e.preventDefault()} onclick={() => onInline("**", "**", "bold text")}>B</button>
  <button class="toolbar-btn toolbar-btn-emphasis" type="button" title="Italic" aria-label="Italic" onmousedown={(e) => e.preventDefault()} onclick={() => onInline("*", "*", "italic text")}>I</button>
</div>

<script lang="ts">
  import "../app.css";
  import { onMount, onDestroy } from "svelte";
  import { isRunning, onProgress } from "$lib/rpc";
  import { sidecarRunning, progressPercent, progressStage } from "$lib/stores";
  import { page } from "$app/stores";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  let { children } = $props();

  let unlisten: UnlistenFn | null = null;

  onMount(async () => {
    try {
      const running = await isRunning();
      sidecarRunning.set(running);
    } catch {}

    // Single global progress listener — pages use the stores, not their own listeners
    unlisten = await onProgress((data) => {
      progressPercent.set(data.percent);
      progressStage.set(data.stage);
    });
  });

  onDestroy(() => {
    unlisten?.();
  });

  let pathname = $derived($page.url.pathname);

  const nav = [
    { href: "/", icon: "📋", label: "Dashboard" },
    { href: "/patients", icon: "👤", label: "Patients" },
    { href: "/sessions", icon: "📅", label: "Sessions" },
    { href: "/transcribe", icon: "🎙️", label: "Transcribe" },
    { href: "/notes", icon: "📝", label: "Notes" },
    { href: "/settings", icon: "⚙️", label: "Settings" },
  ];

  function isActive(href: string): boolean {
    if (href === "/") return pathname === "/";
    return pathname === href || pathname.startsWith(href + "/");
  }
</script>

<div class="app-shell">
  <aside class="sidebar">
    <div class="sidebar-header">
      <h1>Gist</h1>
      <p>Therapy notes tool</p>
    </div>

    <ul class="sidebar-nav">
      {#each nav as item}
        <li>
          <a href={item.href} class:active={isActive(item.href)}>
            <span class="nav-icon">{item.icon}</span>
            {item.label}
          </a>
        </li>
      {/each}
    </ul>

    <div class="sidebar-footer">
      <div class="sidecar-status">
        {#if $sidecarRunning}
          <span class="status-dot running"></span>
          <span>Sidecar running</span>
        {:else}
          <span class="status-dot stopped"></span>
          <span>Sidecar stopped</span>
        {/if}
      </div>
      {#if $progressStage}
        <div style="margin-top: 6px; font-size: 11px; color: var(--text-muted);">
          {$progressStage} ({$progressPercent}%)
        </div>
      {/if}
    </div>
  </aside>

  <main class="main-content">
    {@render children()}
  </main>
</div>

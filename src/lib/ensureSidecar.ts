import { startSidecar, isRunning } from "./rpc";
import { sidecarRunning } from "./stores";
import { get } from "svelte/store";

export async function ensureSidecar(): Promise<boolean> {
  if (get(sidecarRunning)) return true;
  try {
    const running = await isRunning();
    if (running) {
      sidecarRunning.set(true);
      return true;
    }
    await startSidecar();
    sidecarRunning.set(true);
    return true;
  } catch (e) {
    console.error("Failed to start sidecar:", e);
    return false;
  }
}

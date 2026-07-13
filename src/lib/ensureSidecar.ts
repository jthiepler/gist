import { startSidecar, isRunning } from "./rpc";
import { sidecarRunning } from "./stores";

export async function ensureSidecar(): Promise<boolean> {
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
    sidecarRunning.set(false);
    return false;
  }
}

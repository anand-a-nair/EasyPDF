// Frontend entry point.
//
// No framework, deliberately — see ideas/03-tech-decisions.md (TD-006). The
// v1 UI is a canvas, a toolbar, and a few dialogs; a framework would cost
// startup time and bundle size against the budget for very little benefit.

import { invoke } from "@tauri-apps/api/core";

interface CoreStatus {
  readonly version: string;
  readonly pageCount: number;
}

interface WorkerStatus {
  readonly running: boolean;
  readonly sandboxed: boolean;
  readonly detail: string;
  readonly memoryCapped: boolean;
}

type Level = "ok" | "warn" | "none";

function setStatus(id: string, text: string, level: Level): void {
  const element = document.getElementById(id);
  if (element === null) return;
  element.textContent = text;
  element.classList.toggle("ok", level === "ok");
  element.classList.toggle("warn", level === "warn");
}

async function main(): Promise<void> {
  setStatus("shell-status", "running", "ok");

  try {
    // Proves the IPC boundary works end to end: TypeScript to Tauri to the
    // easypdf-core crate and back.
    const status = await invoke<CoreStatus>("core_status");
    setStatus(
      "core-status",
      `v${status.version} — model reachable (${status.pageCount}-page fixture)`,
      "ok",
    );
  } catch (error) {
    setStatus("core-status", `unreachable: ${String(error)}`, "none");
  }

  try {
    const worker = await invoke<WorkerStatus>("worker_status");

    if (!worker.running) {
      setStatus("worker-status", worker.detail, "none");
    } else if (worker.sandboxed) {
      // The memory gap is real on macOS and worth showing rather than hiding
      // behind a checkmark.
      const memory = worker.memoryCapped
        ? "memory capped"
        : "no kernel memory cap on this platform";
      setStatus("worker-status", `${worker.detail}, ${memory}`, "ok");
    } else {
      // Never quietly: an unconfined worker handles untrusted input with
      // ordinary user privileges.
      setStatus("worker-status", worker.detail, "warn");
    }
  } catch (error) {
    setStatus("worker-status", `unreachable: ${String(error)}`, "none");
  }
}

void main();

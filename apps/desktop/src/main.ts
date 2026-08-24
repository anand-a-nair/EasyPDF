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

function setStatus(id: string, text: string, ok: boolean): void {
  const element = document.getElementById(id);
  if (element === null) return;
  element.textContent = text;
  element.classList.toggle("ok", ok);
}

async function main(): Promise<void> {
  setStatus("shell-status", "running", true);

  try {
    // Proves the IPC boundary works end to end: TypeScript to Tauri to the
    // easypdf-core crate and back.
    const status = await invoke<CoreStatus>("core_status");
    setStatus(
      "core-status",
      `v${status.version} — model reachable (${status.pageCount}-page fixture)`,
      true,
    );
  } catch (error) {
    setStatus("core-status", `unreachable: ${String(error)}`, false);
  }
}

void main();

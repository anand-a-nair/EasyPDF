// The toolbar.
//
// Reacts to store notifications rather than being told to update. Previously
// every mutation had to remember to call `updateChrome()`; a feature that
// forgot left the toolbar quietly stale, and there was no way to notice except
// by looking.

import { commands } from "../ipc.js";
import { state, subscribe } from "../state.js";
import { goToHit } from "../features/search.js";
import { panelNames, togglePanel } from "./panels.js";

function element<T extends HTMLElement>(id: string): T | null {
  return document.getElementById(id) as T | null;
}

function setText(id: string, text: string): void {
  const target = element(id);
  if (target !== null) target.textContent = text;
}

function setDisabled(id: string, disabled: boolean): void {
  const target = element<HTMLButtonElement>(id);
  if (target !== null) target.disabled = disabled;
}

function setActive(id: string, active: boolean): void {
  element(id)?.classList.toggle("active", active);
}

export function refreshToolbar(): void {
  const info = state.document;
  document.body.classList.toggle("has-document", info !== null);

  setText("page-indicator", info === null ? "— / —" : `${state.page + 1} / ${info.pageCount}`);
  setText("zoom-indicator", `${Math.round(state.zoom * 100)}%`);
  setText("doc-name", info?.name ?? "");

  setDisabled("prev", info === null || state.page === 0);
  setDisabled("next", info === null || state.page >= info.pageCount - 1);

  setActive("zoom-fit", state.zoomMode === "fit-page");
  setActive("zoom-fit-width", state.zoomMode === "fit-width");
  setActive("view-page", state.viewMode === "page");
  setActive("view-scroll", state.viewMode === "scroll");

  for (const name of panelNames()) {
    setActive(`panel-${name}`, state.panel === name);
  }

  refreshSearchChrome();
}

function refreshSearchChrome(): void {
  const { query, hits, current, truncated } = state.search;

  setText(
    "search-count",
    query === ""
      ? ""
      : hits.length === 0
        ? "no matches"
        : `${current + 1} of ${truncated ? `${hits.length}+` : hits.length}`,
  );

  setDisabled("search-prev", hits.length === 0);
  setDisabled("search-next", hits.length === 0);
}

/** Builds a toolbar button per registered panel, so panels are self-declaring. */
export function buildPanelButtons(titles: Map<string, string>): void {
  const group = element("panel-buttons");
  if (group === null) return;

  group.replaceChildren();
  for (const [name, title] of titles) {
    const button = document.createElement("button");
    button.type = "button";
    button.id = `panel-${name}`;
    button.textContent = title;
    button.addEventListener("click", () => togglePanel(name));
    group.append(button);
  }
}

export function wireSearchBox(
  onQuery: (query: string) => void,
  debounceMs: number,
): void {
  const input = element<HTMLInputElement>("search-input");
  if (input === null) return;

  let timer: number | undefined;
  input.addEventListener("input", () => {
    // Debounced: searching a long document on every keystroke would keep the
    // worker permanently busy and make typing feel sticky.
    window.clearTimeout(timer);
    timer = window.setTimeout(() => onQuery(input.value), debounceMs);
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      goToHit(state.search.current + (event.shiftKey ? -1 : 1));
    }
  });
}

export function clearSearchBox(): void {
  const input = element<HTMLInputElement>("search-input");
  if (input !== null) input.value = "";
}

/**
 * Shows how well the worker confined itself, and whether it has an engine.
 *
 * Surfaced rather than assumed: a worker running with ordinary user privileges,
 * or with no PDF engine at all, is something the user deserves to know about.
 */
export async function refreshWorkerBadge(onError: (message: string) => void): Promise<void> {
  const badge = element("sandbox-badge");
  if (badge === null) return;

  try {
    const worker = await commands.workerStatus();

    // A packaging fault takes precedence: the user should hear about it before
    // picking a file, not after.
    if (worker.running && !worker.engineAvailable) {
      badge.textContent = "no PDF engine";
      badge.title = "The PDF engine is missing from this build.";
      badge.classList.add("warn");
      onError("The PDF engine is missing from this build; documents cannot be opened.");
      return;
    }

    badge.textContent = worker.sandboxed ? "sandboxed" : "NOT sandboxed";
    badge.title = worker.detail;
    badge.classList.toggle("warn", !worker.sandboxed);
  } catch {
    badge.textContent = "worker unavailable";
    badge.classList.add("warn");
  }
}

/** Keeps the toolbar in step with the store. */
export function connectToolbar(): void {
  subscribe(() => refreshToolbar());
}

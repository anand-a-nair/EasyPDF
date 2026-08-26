// Sidebar panels.
//
// The outline was a hard-coded `<aside>` with a hard-coded toggle. Thumbnails,
// annotations, signatures and attachments all want the same slot, and only one
// should be visible at a time.
//
// A panel owns its own DOM. The registry handles mounting, switching and the
// toolbar buttons, so a feature adds a panel without touching layout code.

import { notify, state } from "../state.js";

export interface Panel {
  readonly name: string;
  /** Label for the toolbar button. */
  readonly title: string;
  /** Builds the panel's content once, on first show. */
  mount(container: HTMLElement): void;
  /** Called each time the panel becomes visible, to refresh it. */
  refresh?(): void;
}

const panels = new Map<string, Panel>();
const mounted = new Set<string>();

let container: HTMLElement | null = null;

export function initPanels(element: HTMLElement): void {
  container = element;
}

export function registerPanel(panel: Panel): void {
  panels.set(panel.name, panel);
}

/** Shows a panel, or hides the sidebar when passed the one already open. */
export function togglePanel(name: string): void {
  state.panel = state.panel === name ? null : name;
  renderPanel();
  notify("panel");
}

/** Reveals one panel section and hides the rest. */
function showOnly(container: HTMLElement, name: string): void {
  for (const section of Array.from(container.children)) {
    const element = section as HTMLElement;
    element.hidden = element.dataset["panel"] !== name;
  }
}

export function renderPanel(): void {
  if (container === null) return;

  const name = state.panel;
  if (name === null) {
    container.hidden = true;
    return;
  }

  const panel = panels.get(name);
  if (panel === undefined) {
    container.hidden = true;
    return;
  }

  // Each panel keeps its own section, so switching panels does not throw away
  // and rebuild DOM that is about to be shown again.
  showOnly(container, name);

  if (!mounted.has(name)) {
    const section = document.createElement("div");
    section.dataset["panel"] = name;
    section.className = "panel";
    container.append(section);
    panel.mount(section);
    mounted.add(name);

    showOnly(container, name);
  }

  // Shown *before* refreshing, and layout forced, so a panel that measures its
  // own visible area sees real geometry. Refreshing while still hidden gives
  // every element zero size — the thumbnail panel read that as "everything is
  // on screen" and rendered all two hundred pages.
  container.hidden = false;
  void container.offsetHeight;

  panel.refresh?.();
}

export function panelNames(): string[] {
  return [...panels.keys()];
}

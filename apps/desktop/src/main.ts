// Composition root.
//
// Wires the pieces together and holds no logic of its own. Everything else
// lives in a module with one job: the store in state.ts, typed commands in
// ipc.ts, pixels in viewer/, and each user-facing capability in features/.
//
// The structure exists because Phase 2 adds annotations, page operations,
// form filling and undo — each of which draws on pages, handles pointer
// events, or occupies the sidebar. Those attach through the layer, tool and
// panel registries rather than by editing the viewer.
//
// See ideas/13-frontend-architecture.md. No framework: TD-006 still holds.

import { notify, state } from "./state.js";
import { initPanels, renderPanel } from "./ui/panels.js";
import {
  buildPanelButtons,
  clearSearchBox,
  connectToolbar,
  refreshToolbar,
  refreshWorkerBadge,
  wireSearchBox,
} from "./ui/toolbar.js";
import { askForPassword } from "./ui/dialogs.js";
import { initViewer, invalidate, updateVisiblePages } from "./viewer/viewer.js";
import { setActiveTool } from "./viewer/tools.js";
import { openDocument } from "./features/document.js";
import { applyFit, goToPage, rotate, setViewMode, stepZoom } from "./features/navigation.js";
import { goToHit, registerSearchLayer, runSearch } from "./features/search.js";
import {
  clearSelection,
  copySelection,
  registerSelection,
  selectedText,
  selectWholePage,
} from "./features/selection.js";
import { registerOutlinePanel } from "./features/outline.js";
import { registerThumbnailPanel } from "./features/thumbnails.js";

const SEARCH_DEBOUNCE_MS = 250;
const RESIZE_DEBOUNCE_MS = 120;
const SCROLL_DEBOUNCE_MS = 80;

function element(id: string): HTMLElement {
  const found = document.getElementById(id);
  if (found === null) throw new Error(`missing element: ${id}`);
  return found;
}

const errorBar = element("error");

function showError(message: string): void {
  errorBar.textContent = message;
  errorBar.hidden = false;
}

function clearError(): void {
  errorBar.hidden = true;
}

/** Surfaces anything that escapes a handler, so nothing fails silently. */
function catchStrayFailures(): void {
  window.addEventListener("unhandledrejection", (event) => {
    showError(`Unexpected failure: ${String(event.reason)}`);
  });
  window.addEventListener("error", (event) => {
    showError(`Unexpected error: ${event.message}`);
  });
}

function debounce(fn: () => void, ms: number): () => void {
  let timer: number | undefined;
  return () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(fn, ms);
  };
}

/** Held for the process lifetime: an unreferenced ResizeObserver stops firing. */
let viewportObserver: ResizeObserver | null = null;

function wireUp(): void {
  const viewport = element("viewport");

  element("open").addEventListener("click", () => void open());
  element("open-empty").addEventListener("click", () => void open());
  element("prev").addEventListener("click", () => goToPage(state.page - 1));
  element("next").addEventListener("click", () => goToPage(state.page + 1));
  element("zoom-in").addEventListener("click", () => stepZoom(1));
  element("zoom-out").addEventListener("click", () => stepZoom(-1));
  element("zoom-fit").addEventListener("click", () => void applyFit("fit-page"));
  element("zoom-fit-width").addEventListener("click", () => void applyFit("fit-width"));
  element("rotate-left").addEventListener("click", () => rotate(-1));
  element("rotate-right").addEventListener("click", () => rotate(1));
  element("view-page").addEventListener("click", () => setViewMode("page"));
  element("view-scroll").addEventListener("click", () => setViewMode("scroll"));
  element("search-next").addEventListener("click", () => goToHit(state.search.current + 1));
  element("search-prev").addEventListener("click", () => goToHit(state.search.current - 1));

  wireSearchBox((query) => void runSearch(query, showError), SEARCH_DEBOUNCE_MS);

  viewport.addEventListener("scroll", debounce(updateVisiblePages, SCROLL_DEBOUNCE_MS), {
    passive: true,
  });

  // Re-fit on resize. This is why zoom *mode* is stored rather than just the
  // number: without it a fitted page stays sized to the window it was fitted to.
  const onResize = debounce(() => {
    if (state.zoomMode === "manual") invalidate(false);
    else void applyFit();
  }, RESIZE_DEBOUNCE_MS);

  window.addEventListener("resize", onResize);
  viewportObserver = new ResizeObserver(onResize);
  viewportObserver.observe(viewport);

  document.addEventListener("keydown", handleKey);
}

function handleKey(event: KeyboardEvent): void {
  if (state.document === null) return;

  const modifier = event.metaKey || event.ctrlKey;

  if (modifier && event.key === "c") {
    if (selectedText() !== "") {
      event.preventDefault();
      void copySelection(showError);
    }
    return;
  }

  if (modifier && event.key === "a") {
    event.preventDefault();
    void selectWholePage(showError);
    return;
  }

  if (modifier && event.key === "f") {
    event.preventDefault();
    (document.getElementById("search-input") as HTMLInputElement | null)?.select();
    return;
  }

  if (event.key === "Escape") {
    clearSelection();
    return;
  }

  // Don't hijack keys while the user is typing in the find box.
  if (document.activeElement === document.getElementById("search-input")) return;

  switch (event.key) {
    case "ArrowRight":
    case "PageDown":
      goToPage(state.page + 1);
      break;
    case "ArrowLeft":
    case "PageUp":
      goToPage(state.page - 1);
      break;
    case "Home":
      goToPage(0);
      break;
    case "End":
      goToPage(state.document.pageCount - 1);
      break;
    default:
      break;
  }
}

async function open(): Promise<void> {
  clearSearchBox();
  clearError();
  await openDocument(askForPassword, showError);
  renderPanel();
}

function start(): void {
  catchStrayFailures();

  initViewer(element("viewport"), element("pages"), showError);
  initPanels(element("sidebar"));

  // Features attach themselves. Adding one means registering here, not editing
  // the viewer or the toolbar.
  registerSearchLayer();
  registerSelection(showError);
  registerOutlinePanel();
  registerThumbnailPanel();
  setActiveTool("select");

  buildPanelButtons(
    new Map([
      ["thumbnails", "Pages"],
      ["outline", "Outline"],
    ]),
  );

  connectToolbar();
  wireUp();
  refreshToolbar();

  // Panels refresh when the document or page changes.
  const refreshOnChange = new Set(["document", "page", "outline"]);
  import("./state.js").then(({ subscribe }) => {
    subscribe((change) => {
      if (refreshOnChange.has(change) && state.panel !== null) renderPanel();
    });
  });

  void refreshWorkerBadge(showError);
  notify("document");
}

start();

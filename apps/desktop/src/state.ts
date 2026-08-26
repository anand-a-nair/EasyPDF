// The store: all shared state, and a way to hear about changes.
//
// Not a framework — about sixty lines. But it replaces the pattern the UI grew
// into, where every mutation had to remember to call `updateChrome()` and every
// new feature meant hunting down the call sites that needed to know. That
// pattern does not survive Phase 2, where a page can change because of an
// annotation, an undo, a page reorder, or a tool switch.
//
// See ideas/13-frontend-architecture.md.

import type { DocumentInfo, OutlineEntry, PageDimensions, SearchHit, TextLayout } from "./ipc.js";

/** How the document is laid out. */
export type ViewMode = "page" | "scroll";

/**
 * How the zoom level is decided.
 *
 * The *intent* is stored, not just the number: a fitted zoom has to be
 * recomputed when the viewport changes, and a bare number cannot say whether
 * it should be.
 */
export type ZoomMode = "manual" | "fit-page" | "fit-width";

export interface SelectionState {
  page: number;
  /** Inclusive character range; -1 when nothing is selected. */
  from: number;
  to: number;
  dragging: boolean;
}

export interface SearchState {
  query: string;
  hits: readonly SearchHit[];
  current: number;
  truncated: boolean;
}

export interface AppState {
  document: DocumentInfo | null;
  page: number;
  zoom: number;
  zoomMode: ZoomMode;
  viewMode: ViewMode;
  rotation: number;
  /** Invalidates in-flight renders when the view changes underneath them. */
  generation: number;
  pageSizes: Map<number, PageDimensions>;
  layouts: Map<number, TextLayout>;
  outline: readonly OutlineEntry[];
  selection: SelectionState;
  search: SearchState;
  /** Which pointer tool is active. Phase 2 adds highlight, ink, note, shapes. */
  tool: string;
  /** Which sidebar panel is open, or null. */
  panel: string | null;
}

/** Everything a change can be about. Listeners subscribe to what they care about. */
export type Change =
  | "document" // opened, closed, or replaced
  | "page" // current page changed
  | "view" // zoom, rotation, or view mode
  | "search"
  | "selection"
  | "outline"
  | "tool"
  | "panel";

type Listener = (change: Change) => void;

function initialState(): AppState {
  return {
    document: null,
    page: 0,
    zoom: 1,
    zoomMode: "manual",
    viewMode: "page",
    rotation: 0,
    generation: 0,
    pageSizes: new Map(),
    layouts: new Map(),
    outline: [],
    selection: { page: -1, from: -1, to: -1, dragging: false },
    search: { query: "", hits: [], current: -1, truncated: false },
    tool: "select",
    panel: null,
  };
}

export const state: AppState = initialState();

const listeners = new Set<Listener>();

/** Subscribes to changes. Returns an unsubscribe function. */
export function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Announces a change.
 *
 * A listener that throws must not stop the others from hearing: a broken
 * toolbar should not take the viewer down with it.
 */
export function notify(change: Change): void {
  for (const listener of [...listeners]) {
    try {
      listener(change);
    } catch (error) {
      console.error(`listener failed on "${change}":`, error);
    }
  }
}

/** Resets everything document-scoped. Called when a document opens or closes. */
export function resetDocumentState(): void {
  state.page = 0;
  state.zoom = 1;
  state.zoomMode = "manual";
  state.rotation = 0;
  state.pageSizes.clear();
  state.layouts.clear();
  state.outline = [];
  state.selection = { page: -1, from: -1, to: -1, dragging: false };
  state.search = { query: "", hits: [], current: -1, truncated: false };
}

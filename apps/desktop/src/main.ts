// Frontend entry point.
//
// No framework, deliberately — see ideas/03-tech-decisions.md (TD-006). A
// bundler is required though: bare module specifiers cannot be resolved by a
// webview, which once shipped a completely inert UI (D-021).
//
// Two things shape the design here, both from ideas/04-performance-budget.md:
//
//   Feel fast.  Paint something immediately, never block on a full render,
//               and never let an older render overwrite a newer one.
//   Stay small. Only pages near the viewport hold pixels. Everything else
//               releases its backing store, so a 500-page document costs
//               roughly what a 5-page one does.

import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

interface DocumentInfo {
  readonly name: string;
  readonly pageCount: number;
  readonly encrypted: boolean;
}

interface PageDimensions {
  readonly width: number;
  readonly height: number;
}

interface TextRect {
  readonly left: number;
  readonly bottom: number;
  readonly right: number;
  readonly top: number;
}

interface SearchHit {
  readonly page: number;
  readonly rects: readonly TextRect[];
}

interface SearchResults {
  readonly hits: readonly SearchHit[];
  readonly truncated: boolean;
}

interface WorkerStatus {
  readonly running: boolean;
  readonly sandboxed: boolean;
  readonly detail: string;
  readonly memoryCapped: boolean;
}

/** How the document is laid out. */
type ViewMode = "page" | "scroll";

/**
 * How the zoom level is decided.
 *
 * Storing the *intent* rather than only the resulting number is what makes fit
 * survive a window resize: a fitted zoom has to be recomputed when the viewport
 * changes, and a number alone cannot tell us whether it should be.
 */
type ZoomMode = "manual" | "fit-page" | "fit-width";

/** Zoom stops the +/- buttons step through. */
const ZOOM_STOPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4] as const;

const MIN_ZOOM = 0.1;
const MAX_ZOOM = 8;

/** Scale of the instant first paint, before the sharp render arrives. */
const PREVIEW_SCALE = 0.25;

/** Padding around a page inside the viewport, in CSS pixels. */
const VIEWPORT_PADDING = 48;

/** Gap between pages in scroll mode, in CSS pixels. */
const PAGE_GAP = 16;

/**
 * How many pages either side of the viewport keep their pixels.
 *
 * The whole memory story in scroll mode. Larger means less flicker when
 * scrolling fast; smaller means less memory. Two is enough to cover a normal
 * scroll gesture without holding a document's worth of bitmaps.
 */
const RETAINED_PAGE_RADIUS = 2;

interface Slot {
  readonly root: HTMLElement;
  readonly canvas: HTMLCanvasElement;
  readonly highlights: HTMLCanvasElement;
  /** Identifies what is currently painted, so identical work is skipped. */
  renderedKey: string | null;
  /** True once any pixels have been painted, used to skip the preview pass. */
  hasPixels: boolean;
}

const state = {
  document: null as DocumentInfo | null,
  page: 0,
  zoom: 1,
  zoomMode: "manual" as ZoomMode,
  viewMode: "page" as ViewMode,
  /** Invalidates in-flight renders when the view changes underneath them. */
  generation: 0,
  pageSizes: new Map<number, PageDimensions>(),
  slots: new Map<number, Slot>(),
  search: {
    query: "",
    hits: [] as readonly SearchHit[],
    current: -1,
    truncated: false,
  },
};

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) throw new Error(`missing element: ${id}`);
  return found as T;
}

const viewport = element("viewport");
const pagesContainer = element("pages");
const errorBar = element("error");

function showError(message: string): void {
  errorBar.textContent = message;
  errorBar.hidden = false;
}

function clearError(): void {
  errorBar.hidden = true;
}

// --- page geometry ---------------------------------------------------------

/**
 * A page's size in points, fetched once and remembered.
 *
 * Worth caching: it is an IPC round trip, it never changes for a given
 * document, and the layout needs it for every page in scroll mode.
 */
async function pageSize(page: number): Promise<PageDimensions> {
  const known = state.pageSizes.get(page);
  if (known !== undefined) return known;

  const size = await invoke<PageDimensions>("page_size", { page });
  state.pageSizes.set(page, size);
  return size;
}

/**
 * The best size estimate available without waiting.
 *
 * Layout must be stable before pixels arrive, or every page that finishes
 * rendering shoves the ones below it and the scroll position jumps. Most
 * documents are uniform, so the first known page is a good estimate, and it is
 * corrected as real sizes arrive.
 */
function estimatedSize(page: number): PageDimensions {
  return (
    state.pageSizes.get(page) ??
    state.pageSizes.values().next().value ?? { width: 612, height: 792 }
  );
}

// --- slots -----------------------------------------------------------------

function createSlot(page: number): Slot {
  const root = document.createElement("div");
  root.className = "page-slot";
  root.dataset["page"] = String(page);

  // A fresh <canvas> defaults to 300x150 and allocates that backing store
  // immediately. Across 200 slots that is ~36 MB held for pages nobody has
  // looked at. Start them at zero and let rendering size them.
  const canvas = document.createElement("canvas");
  canvas.className = "page-canvas";
  canvas.width = 0;
  canvas.height = 0;

  const highlights = document.createElement("canvas");
  highlights.className = "page-highlights";
  highlights.width = 0;
  highlights.height = 0;

  root.append(canvas, highlights);
  return { root, canvas, highlights, renderedKey: null, hasPixels: false };
}

/** Sizes a slot from the page dimensions, without waiting for pixels. */
function layOutSlot(slot: Slot, page: number): void {
  const size = estimatedSize(page);
  const cssWidth = Math.round(size.width * state.zoom);
  const cssHeight = Math.round(size.height * state.zoom);

  slot.root.style.width = `${cssWidth}px`;
  slot.root.style.height = `${cssHeight}px`;
}

/** Frees a slot's pixel buffers while leaving its place in the layout. */
function releaseSlotPixels(slot: Slot): void {
  // Setting a canvas dimension to zero is what actually releases the backing
  // store; hiding it or clearing it does not.
  slot.canvas.width = 0;
  slot.canvas.height = 0;
  slot.highlights.width = 0;
  slot.highlights.height = 0;
  slot.renderedKey = null;
  slot.hasPixels = false;
}

/** Rebuilds the slot list for the current view mode. */
function buildSlots(): void {
  const info = state.document;
  pagesContainer.replaceChildren();
  state.slots.clear();

  if (info === null) return;

  const pages =
    state.viewMode === "scroll"
      ? Array.from({ length: info.pageCount }, (_, index) => index)
      : [state.page];

  for (const page of pages) {
    const slot = createSlot(page);
    layOutSlot(slot, page);
    state.slots.set(page, slot);
    pagesContainer.append(slot.root);
  }

  pagesContainer.hidden = false;
}

// --- rendering -------------------------------------------------------------

function renderKey(page: number): string {
  return `${page}@${(state.zoom * window.devicePixelRatio).toFixed(3)}`;
}

/** Decodes a render response: u32 width, u32 height, then RGBA bytes. */
function decodePage(buffer: ArrayBuffer): ImageData {
  const header = new DataView(buffer, 0, 8);
  const width = header.getUint32(0, true);
  const height = header.getUint32(4, true);
  const pixels = new Uint8ClampedArray(buffer, 8);

  if (pixels.length !== width * height * 4) {
    throw new Error(`page data does not match its ${width}x${height} header`);
  }
  return new ImageData(pixels, width, height);
}

function paintSlot(slot: Slot, image: ImageData, cssWidth: number, cssHeight: number): void {
  slot.canvas.width = image.width;
  slot.canvas.height = image.height;
  slot.canvas.style.width = `${cssWidth}px`;
  slot.canvas.style.height = `${cssHeight}px`;

  const context = slot.canvas.getContext("2d");
  if (context === null) return;
  context.putImageData(image, 0, 0);
  slot.hasPixels = true;
}

/**
 * Serialises rendering.
 *
 * The worker renders one page at a time regardless, so firing twenty requests
 * at a fast scroll only builds a queue of work that is already stale by the
 * time it runs. One in flight, newest intent wins.
 */
let renderChain: Promise<void> = Promise.resolve();

function scheduleRender(page: number): void {
  const generation = state.generation;
  // `.catch` before `.then` matters: a rejected chain skips every subsequent
  // `.then` forever, so a single failed render would silently stop all
  // rendering for the rest of the session.
  renderChain = renderChain.catch(() => undefined).then(async () => {
    if (generation !== state.generation) return;
    // Re-check on the way out of the queue, not just on the way in. A page
    // queued while the viewport was elsewhere is wasted work by the time its
    // turn comes, and during a fast scroll that is most of the queue.
    if (!isPageWanted(page)) return;
    await renderSlot(page, generation);
  });
}

/** Whether a page is close enough to the viewport to be worth pixels. */
function isPageWanted(page: number): boolean {
  if (state.viewMode === "page") return page === state.page;

  const { first, last } = visiblePageRange();
  return page >= first - RETAINED_PAGE_RADIUS && page <= last + RETAINED_PAGE_RADIUS;
}

async function renderSlot(page: number, generation: number): Promise<void> {
  const slot = state.slots.get(page);
  if (slot === undefined) return;

  const key = renderKey(page);
  if (slot.renderedKey === key) return;

  const deviceZoom = state.zoom * window.devicePixelRatio;

  try {
    const size = await pageSize(page);
    if (generation !== state.generation) return;

    const cssWidth = Math.round(size.width * state.zoom);
    const cssHeight = Math.round(size.height * state.zoom);
    slot.root.style.width = `${cssWidth}px`;
    slot.root.style.height = `${cssHeight}px`;

    // A cheap blurry pass, but only when the slot is empty. Once a page has
    // pixels, going via a low-resolution version first is a visible downgrade
    // rather than an improvement.
    if (!slot.hasPixels) {
      const preview = await invoke<ArrayBuffer>("render_page", {
        page,
        zoom: Math.max(deviceZoom * PREVIEW_SCALE, 0.05),
      });
      if (generation !== state.generation) return;
      paintSlot(slot, decodePage(preview), cssWidth, cssHeight);
    }

    const full = await invoke<ArrayBuffer>("render_page", { page, zoom: deviceZoom });
    if (generation !== state.generation) return;

    paintSlot(slot, decodePage(full), cssWidth, cssHeight);
    slot.renderedKey = key;
    drawHighlights(page);
    clearError();
  } catch (error) {
    if (generation === state.generation) {
      showError(`Could not render page ${page + 1}: ${String(error)}`);
    }
  }
}

/**
 * Renders what is on screen and releases what is not.
 *
 * The memory story: only pages within RETAINED_PAGE_RADIUS of the viewport
 * hold pixels, so scrolling a 500-page document costs about what a handful of
 * pages costs.
 */
function updateVisiblePages(): void {
  if (state.document === null) return;

  if (state.viewMode === "page") {
    scheduleRender(state.page);
    return;
  }

  const visible = visiblePageRange();
  for (const [page, slot] of state.slots) {
    const near =
      page >= visible.first - RETAINED_PAGE_RADIUS &&
      page <= visible.last + RETAINED_PAGE_RADIUS;

    if (near) {
      scheduleRender(page);
    } else if (slot.hasPixels) {
      releaseSlotPixels(slot);
    }
  }

  if (visible.first !== state.page) {
    state.page = visible.first;
    updateChrome();
  }
}

function visiblePageRange(): { first: number; last: number } {
  const top = viewport.scrollTop;
  const bottom = top + viewport.clientHeight;

  let first = 0;
  let last = 0;
  let found = false;

  for (const [page, slot] of state.slots) {
    const slotTop = slot.root.offsetTop;
    const slotBottom = slotTop + slot.root.offsetHeight;
    if (slotBottom >= top && slotTop <= bottom) {
      if (!found) {
        first = page;
        found = true;
      }
      last = page;
    }
  }

  return found ? { first, last } : { first: 0, last: 0 };
}

// --- highlights ------------------------------------------------------------

/**
 * Draws search highlights over one page.
 *
 * PDF coordinates start at the bottom-left and are measured in points; canvas
 * coordinates start top-left in device pixels. Getting that flip wrong puts
 * every highlight on the opposite side of the page, which looks deliberate
 * enough to go unnoticed — so it is asserted numerically in the harness.
 */
function drawHighlights(page: number): void {
  const slot = state.slots.get(page);
  if (slot === undefined) return;

  const hits = state.search.hits.filter((hit) => hit.page === page);
  const size = state.pageSizes.get(page);

  // Only pay for an overlay when there is something to draw on it. A
  // full-size second canvas per page doubles the memory of every rendered
  // page, and most of the time no search is running at all.
  if (hits.length === 0 || size === undefined || size.width <= 0 || size.height <= 0) {
    slot.highlights.width = 0;
    slot.highlights.height = 0;
    return;
  }

  // Match the page canvas exactly.
  slot.highlights.width = slot.canvas.width;
  slot.highlights.height = slot.canvas.height;
  slot.highlights.style.width = slot.canvas.style.width;
  slot.highlights.style.height = slot.canvas.style.height;

  const context = slot.highlights.getContext("2d");
  if (context === null) return;
  context.clearRect(0, 0, slot.highlights.width, slot.highlights.height);

  const scaleX = slot.highlights.width / size.width;
  const scaleY = slot.highlights.height / size.height;

  state.search.hits.forEach((hit, index) => {
    if (hit.page !== page) return;

    context.fillStyle =
      index === state.search.current ? "rgba(255, 145, 0, 0.45)" : "rgba(255, 213, 0, 0.35)";

    for (const rect of hit.rects) {
      const width = (rect.right - rect.left) * scaleX;
      const height = (rect.top - rect.bottom) * scaleY;
      if (width <= 0 || height <= 0) continue;
      context.fillRect(rect.left * scaleX, (size.height - rect.top) * scaleY, width, height);
    }
  });
}

function redrawAllHighlights(): void {
  for (const page of state.slots.keys()) drawHighlights(page);
}

// --- view and zoom ---------------------------------------------------------

/** Discards in-flight work and rebuilds the view. */
function invalidate(rebuild: boolean): void {
  state.generation += 1;
  if (rebuild) {
    buildSlots();
  } else {
    for (const [page, slot] of state.slots) {
      slot.renderedKey = null;
      layOutSlot(slot, page);
    }
  }

  // Force layout before deciding what is visible. Measuring straight after
  // building the slots reads every offset as zero, so every page looks
  // on-screen and a 200-page document queues 200 renders.
  //
  // Reading offsetHeight flushes pending layout synchronously. This used to be
  // a requestAnimationFrame callback, which is worse in two ways: it does not
  // fire at all in a headless browser, and in a real window it does not fire
  // while the window is minimised — so a document opened in the background
  // would stay blank until something else triggered a render.
  void pagesContainer.offsetHeight;
  updateVisiblePages();
}

async function setZoom(zoom: number, mode: ZoomMode): Promise<void> {
  state.zoom = Math.max(MIN_ZOOM, Math.min(zoom, MAX_ZOOM));
  state.zoomMode = mode;
  updateChrome();
  invalidate(false);
}

/** Multiplier used when the zoom is already outside the fixed stops. */
const ZOOM_STEP_FACTOR = 1.25;

function stepZoom(direction: 1 | -1): void {
  const current = state.zoom;
  const stops = direction === 1 ? [...ZOOM_STOPS] : [...ZOOM_STOPS].reverse();
  const next = stops.find((stop) =>
    direction === 1 ? stop > current + 0.001 : stop < current - 0.001,
  );

  // Fitting a small page can land above the largest stop, and then there is no
  // "next" stop to step to — zoom in would silently do nothing. Fall back to a
  // relative step so the button always moves.
  const target =
    next ?? (direction === 1 ? current * ZOOM_STEP_FACTOR : current / ZOOM_STEP_FACTOR);

  // A manual zoom cancels fit: the user has taken over.
  void setZoom(target, "manual");
}

/**
 * Applies the current fit mode.
 *
 * Called on resize as well as on demand, which is what makes fit *stay* fitted
 * when the window changes. Storing only the resulting number would leave the
 * page at whatever size fitted the old window.
 */
async function applyFit(mode: ZoomMode = state.zoomMode): Promise<void> {
  if (state.document === null || mode === "manual") return;

  const available = {
    width: viewport.clientWidth - VIEWPORT_PADDING,
    height: viewport.clientHeight - VIEWPORT_PADDING,
  };

  // Before first layout, or in a window too small to mean anything, fitting
  // would collapse to the minimum zoom. Keeping the current zoom is less
  // surprising.
  if (available.width < 50 || available.height < 50) return;

  try {
    const size = await pageSize(state.page);
    if (size.width <= 0 || size.height <= 0) return;

    const scale =
      mode === "fit-width"
        ? available.width / size.width
        : Math.min(available.width / size.width, available.height / size.height);

    await setZoom(scale, mode);
  } catch (error) {
    showError(`Could not fit page: ${String(error)}`);
  }
}

function setViewMode(mode: ViewMode): void {
  if (state.viewMode === mode) return;
  state.viewMode = mode;

  element("view-page").classList.toggle("active", mode === "page");
  element("view-scroll").classList.toggle("active", mode === "scroll");

  invalidate(true);
  if (mode === "scroll") scrollToPage(state.page);
}

// --- navigation ------------------------------------------------------------

function scrollToPage(page: number): void {
  const slot = state.slots.get(page);
  if (slot === undefined) return;
  viewport.scrollTo({ top: slot.root.offsetTop - PAGE_GAP, behavior: "auto" });
}

function goToPage(page: number): void {
  const info = state.document;
  if (info === null) return;

  const clamped = Math.max(0, Math.min(page, info.pageCount - 1));
  if (clamped === state.page && state.viewMode === "page") return;

  state.page = clamped;
  updateChrome();

  if (state.viewMode === "scroll") {
    scrollToPage(clamped);
    updateVisiblePages();
  } else {
    invalidate(true);
  }
}

// --- chrome ----------------------------------------------------------------

function updateChrome(): void {
  const info = state.document;
  document.body.classList.toggle("has-document", info !== null);

  element("page-indicator").textContent =
    info === null ? "— / —" : `${state.page + 1} / ${info.pageCount}`;
  element("zoom-indicator").textContent = `${Math.round(state.zoom * 100)}%`;
  element("doc-name").textContent = info?.name ?? "";

  element<HTMLButtonElement>("prev").disabled = info === null || state.page === 0;
  element<HTMLButtonElement>("next").disabled =
    info === null || state.page >= info.pageCount - 1;

  element("zoom-fit").classList.toggle("active", state.zoomMode === "fit-page");
  element("zoom-fit-width").classList.toggle("active", state.zoomMode === "fit-width");

  updateSearchChrome();
}

function updateSearchChrome(): void {
  const { query, hits, current, truncated } = state.search;
  const count = element("search-count");

  if (query === "") {
    count.textContent = "";
  } else if (hits.length === 0) {
    count.textContent = "no matches";
  } else {
    count.textContent = `${current + 1} of ${truncated ? `${hits.length}+` : hits.length}`;
  }

  const noHits = hits.length === 0;
  element<HTMLButtonElement>("search-prev").disabled = noHits;
  element<HTMLButtonElement>("search-next").disabled = noHits;
}

// --- search ----------------------------------------------------------------

async function runSearch(query: string): Promise<void> {
  state.search.query = query;

  if (query === "" || state.document === null) {
    state.search.hits = [];
    state.search.current = -1;
    state.search.truncated = false;
    updateSearchChrome();
    redrawAllHighlights();
    return;
  }

  try {
    const results = await invoke<SearchResults>("search", { query, matchCase: false });
    state.search.hits = results.hits;
    state.search.truncated = results.truncated;
    state.search.current = results.hits.length > 0 ? 0 : -1;
    updateSearchChrome();

    if (results.hits.length === 0) {
      // Must redraw explicitly: goToHit returns early with no hits, which
      // previously left the last search's highlights on screen while the
      // toolbar said "no matches".
      redrawAllHighlights();
      return;
    }

    goToHit(0);
  } catch (error) {
    showError(`Search failed: ${String(error)}`);
  }
}

function goToHit(index: number): void {
  const { hits } = state.search;
  if (hits.length === 0) return;

  // Wrapping is what every find box does; stopping dead at the last hit reads
  // as broken.
  const wrapped = ((index % hits.length) + hits.length) % hits.length;
  state.search.current = wrapped;

  const hit = hits[wrapped];
  if (hit === undefined) return;

  updateSearchChrome();

  if (hit.page !== state.page) {
    goToPage(hit.page);
  }
  redrawAllHighlights();
}

// --- document --------------------------------------------------------------

async function openDocument(): Promise<void> {
  // The dialog call belongs inside the try as well: when the capability grant
  // was missing it rejected outside any handler, so the click did nothing at
  // all, with no message anywhere.
  try {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof selected !== "string") return;

    const info = await invoke<DocumentInfo>("open_document", { path: selected });

    state.document = info;
    state.page = 0;
    state.zoom = 1;
    state.zoomMode = "manual";
    state.pageSizes.clear();
    state.search = { query: "", hits: [], current: -1, truncated: false };
    element<HTMLInputElement>("search-input").value = "";

    clearError();
    updateChrome();
    invalidate(true);

    // Fit on open: a page at an arbitrary 100% is rarely what anyone wants
    // first, and it makes the fit mode discoverable.
    await applyFit("fit-page");
  } catch (error) {
    showError(`Could not open document: ${String(error)}`);
  }
}

async function showSandboxStatus(): Promise<void> {
  const badge = element("sandbox-badge");
  try {
    const worker = await invoke<WorkerStatus>("worker_status");
    badge.textContent = worker.sandboxed ? "sandboxed" : "NOT sandboxed";
    badge.title = worker.detail;
    badge.classList.toggle("warn", !worker.sandboxed);
  } catch {
    badge.textContent = "worker unavailable";
    badge.classList.add("warn");
  }
}

// --- wiring ----------------------------------------------------------------

/** Surfaces anything that escapes a handler, so nothing fails silently. */
function catchStrayFailures(): void {
  window.addEventListener("unhandledrejection", (event) => {
    showError(`Unexpected failure: ${String(event.reason)}`);
  });
  window.addEventListener("error", (event) => {
    showError(`Unexpected error: ${event.message}`);
  });
}

/** Held for the process lifetime; see the note where it is assigned. */
let viewportObserver: ResizeObserver | null = null;

function debounce(fn: () => void, ms: number): () => void {
  let timer: number | undefined;
  return () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(fn, ms);
  };
}

function wireUp(): void {
  element("open").addEventListener("click", () => void openDocument());
  element("open-empty").addEventListener("click", () => void openDocument());
  element("prev").addEventListener("click", () => goToPage(state.page - 1));
  element("next").addEventListener("click", () => goToPage(state.page + 1));
  element("zoom-in").addEventListener("click", () => stepZoom(1));
  element("zoom-out").addEventListener("click", () => stepZoom(-1));
  element("zoom-fit").addEventListener("click", () => void applyFit("fit-page"));
  element("zoom-fit-width").addEventListener("click", () => void applyFit("fit-width"));
  element("view-page").addEventListener("click", () => setViewMode("page"));
  element("view-scroll").addEventListener("click", () => setViewMode("scroll"));

  // Scrolling drives rendering in scroll mode. Debounced so a fast flick does
  // not queue a render for every page it passes.
  viewport.addEventListener("scroll", debounce(() => updateVisiblePages(), 80), {
    passive: true,
  });

  // Re-fit on resize. This is the whole reason zoom *mode* is stored rather
  // than just the number: without it, a fitted page stays sized to the window
  // it was fitted to.
  const onResize = debounce(() => {
    if (state.zoomMode === "manual") {
      invalidate(false);
    } else {
      void applyFit();
    }
  }, 120);
  window.addEventListener("resize", onResize);

  // The observer must be held in a variable. An unreferenced ResizeObserver
  // is eligible for collection and simply stops firing — which it did: the
  // viewport went from 980px to 500px wide and the fitted zoom never
  // recomputed.
  viewportObserver = new ResizeObserver(onResize);
  viewportObserver.observe(viewport);

  const searchInput = element<HTMLInputElement>("search-input");
  const runDebouncedSearch = debounce(() => void runSearch(searchInput.value), 250);
  searchInput.addEventListener("input", runDebouncedSearch);
  searchInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      goToHit(state.search.current + (event.shiftKey ? -1 : 1));
    }
  });

  element("search-next").addEventListener("click", () => goToHit(state.search.current + 1));
  element("search-prev").addEventListener("click", () => goToHit(state.search.current - 1));

  document.addEventListener("keydown", (event) => {
    if (state.document === null) return;

    if ((event.metaKey || event.ctrlKey) && event.key === "f") {
      event.preventDefault();
      element<HTMLInputElement>("search-input").select();
      return;
    }

    // Don't hijack keys while the user is typing in the find box.
    if (document.activeElement === element("search-input")) return;

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
  });
}

catchStrayFailures();
wireUp();
updateChrome();
void showSandboxStatus();

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

interface CharBox {
  readonly text: string;
  readonly rect: TextRect;
}

interface TextLayout {
  readonly chars: readonly CharBox[];
  readonly truncated: boolean;
}

interface OutlineEntry {
  readonly title: string;
  readonly depth: number;
  readonly page: number | null;
}

/** Shape of the error `open_document` rejects with. */
interface OpenError {
  readonly needsPassword: boolean;
  readonly message: string;
}

interface WorkerStatus {
  readonly running: boolean;
  readonly sandboxed: boolean;
  readonly detail: string;
  readonly memoryCapped: boolean;
  readonly engineAvailable: boolean;
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
  /** Display rotation in degrees clockwise, applied to every page. */
  rotation: 0,
  outline: [] as readonly OutlineEntry[],
  /** Per-page character geometry, fetched once and reused for every drag. */
  layouts: new Map<number, TextLayout>(),
  selection: {
    page: -1,
    /** Inclusive character range; -1 when nothing is selected. */
    from: -1,
    to: -1,
    dragging: false,
  },
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
  const known =
    state.pageSizes.get(page) ??
    state.pageSizes.values().next().value ?? { width: 612, height: 792 };
  return displayedSize(known);
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
  const slot: Slot = { root, canvas, highlights, renderedKey: null, hasPixels: false };

  root.addEventListener("pointerdown", (event) => {
    // Left button only; a right-click should not wipe the selection.
    if (event.button !== 0) return;
    event.preventDefault();
    root.setPointerCapture(event.pointerId);
    void beginSelection(event, page);
  });
  root.addEventListener("pointermove", (event) => extendSelection(event, page));
  root.addEventListener("pointerup", (event) => {
    state.selection.dragging = false;
    if (root.hasPointerCapture(event.pointerId)) root.releasePointerCapture(event.pointerId);
  });

  return slot;
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
  return `${page}@${(state.zoom * window.devicePixelRatio).toFixed(3)}r${state.rotation}`;
}

/** Page dimensions as displayed, accounting for rotation. */
function displayedSize(size: PageDimensions): PageDimensions {
  const quarterTurn = state.rotation === 90 || state.rotation === 270;
  return quarterTurn ? { width: size.height, height: size.width } : size;
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
    const size = displayedSize(await pageSize(page));
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
        rotation: state.rotation,
      });
      if (generation !== state.generation) return;
      paintSlot(slot, decodePage(preview), cssWidth, cssHeight);
    }

    const full = await invoke<ArrayBuffer>("render_page", {
      page,
      zoom: deviceZoom,
      rotation: state.rotation,
    });
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

  // Hit rectangles are in unrotated page coordinates. Mapping them through a
  // rotation is straightforward but untested, and a highlight in the wrong
  // place is worse than none — so while rotated, neither highlights nor
  // selection are drawn.
  const rotated = state.rotation !== 0;
  const hits = rotated ? [] : state.search.hits.filter((hit) => hit.page === page);
  const selectionRects = rotated ? [] : selectedRects(page);
  const size = state.pageSizes.get(page);

  // Only pay for an overlay when there is something to draw on it. A
  // full-size second canvas per page doubles the memory of every rendered
  // page, and most of the time nothing is highlighted or selected.
  if (
    (hits.length === 0 && selectionRects.length === 0) ||
    size === undefined ||
    size.width <= 0 ||
    size.height <= 0
  ) {
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

  // Selection is drawn last so it reads on top of a search highlight.
  context.fillStyle = "rgba(64, 120, 220, 0.35)";
  for (const rect of selectionRects) {
    const width = (rect.right - rect.left) * scaleX;
    const height = (rect.top - rect.bottom) * scaleY;
    if (width <= 0 || height <= 0) continue;
    context.fillRect(rect.left * scaleX, (size.height - rect.top) * scaleY, width, height);
  }
}

// --- text selection --------------------------------------------------------

/** Fetches a page's character geometry once, then reuses it. */
async function textLayout(page: number): Promise<TextLayout> {
  const known = state.layouts.get(page);
  if (known !== undefined) return known;

  const layout = await invoke<TextLayout>("text_layout", { page });
  state.layouts.set(page, layout);
  return layout;
}

/** Rectangles covering the current selection on `page`. */
function selectedRects(page: number): TextRect[] {
  const { page: selectedPage, from, to } = state.selection;
  if (selectedPage !== page || from < 0 || to < 0) return [];

  const layout = state.layouts.get(page);
  if (layout === undefined) return [];

  const [start, end] = from <= to ? [from, to] : [to, from];
  return layout.chars.slice(start, end + 1).map((character) => character.rect);
}

/** The selected text, or an empty string. */
function selectedText(): string {
  const { page, from, to } = state.selection;
  const layout = state.layouts.get(page);
  if (layout === undefined || from < 0 || to < 0) return "";

  const [start, end] = from <= to ? [from, to] : [to, from];
  return layout.chars
    .slice(start, end + 1)
    .map((character) => character.text)
    .join("");
}

/** Converts a pointer event to page coordinates in points. */
function pointToPageCoords(
  event: PointerEvent,
  slot: Slot,
  page: number,
): { x: number; y: number } | null {
  const size = state.pageSizes.get(page);
  if (size === undefined || size.width <= 0 || size.height <= 0) return null;

  const bounds = slot.root.getBoundingClientRect();
  if (bounds.width <= 0 || bounds.height <= 0) return null;

  const x = ((event.clientX - bounds.left) / bounds.width) * size.width;
  // PDF's origin is bottom-left; the pointer's is top-left.
  const y = size.height - ((event.clientY - bounds.top) / bounds.height) * size.height;
  return { x, y };
}

/**
 * Finds the character nearest a page-space point.
 *
 * Nearest rather than strictly containing: a drag that ends in the gutter or
 * between lines should still select something sensible, not nothing.
 */
function nearestCharIndex(layout: TextLayout, x: number, y: number): number {
  let best = -1;
  let bestDistance = Number.POSITIVE_INFINITY;

  layout.chars.forEach((character, index) => {
    const { left, right, bottom, top } = character.rect;
    // Distance to the rectangle, zero when inside it.
    const dx = x < left ? left - x : x > right ? x - right : 0;
    const dy = y < bottom ? bottom - y : y > top ? y - top : 0;
    const distance = dx * dx + dy * dy;

    if (distance < bestDistance) {
      bestDistance = distance;
      best = index;
    }
  });

  return best;
}

function clearSelection(): void {
  const previous = state.selection.page;
  state.selection = { page: -1, from: -1, to: -1, dragging: false };
  if (previous >= 0) drawHighlights(previous);
}

async function beginSelection(event: PointerEvent, page: number): Promise<void> {
  const slot = state.slots.get(page);
  if (slot === undefined || state.rotation !== 0) return;

  try {
    const layout = await textLayout(page);
    const point = pointToPageCoords(event, slot, page);
    if (point === null) return;

    const index = nearestCharIndex(layout, point.x, point.y);
    if (index < 0) return;

    if (state.selection.page >= 0 && state.selection.page !== page) {
      drawHighlights(state.selection.page);
    }
    state.selection = { page, from: index, to: index, dragging: true };
    drawHighlights(page);
  } catch (error) {
    showError(`Could not prepare text selection: ${String(error)}`);
  }
}

function extendSelection(event: PointerEvent, page: number): void {
  if (!state.selection.dragging || state.selection.page !== page) return;

  const slot = state.slots.get(page);
  const layout = state.layouts.get(page);
  if (slot === undefined || layout === undefined) return;

  const point = pointToPageCoords(event, slot, page);
  if (point === null) return;

  const index = nearestCharIndex(layout, point.x, point.y);
  if (index < 0 || index === state.selection.to) return;

  state.selection.to = index;
  drawHighlights(page);
}

/**
 * Copies the selection.
 *
 * Tries the async clipboard API, then falls back to a hidden textarea. The
 * webview is not always treated as a secure context, and `navigator.clipboard`
 * is simply absent when it is not — a silent no-op on Copy would be worse than
 * the deprecated fallback.
 */
async function copySelection(): Promise<void> {
  const text = selectedText();
  if (text === "") return;

  try {
    if (navigator.clipboard?.writeText !== undefined) {
      await navigator.clipboard.writeText(text);
      return;
    }
  } catch {
    // Fall through to the textarea.
  }

  const scratch = document.createElement("textarea");
  scratch.value = text;
  scratch.setAttribute("readonly", "");
  scratch.style.position = "fixed";
  scratch.style.opacity = "0";
  document.body.append(scratch);
  scratch.select();
  try {
    document.execCommand("copy");
  } catch {
    showError("Could not copy the selection.");
  } finally {
    scratch.remove();
  }
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
    const size = displayedSize(await pageSize(state.page));
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

/** Rotates the whole document by a quarter turn. */
function rotate(direction: 1 | -1): void {
  state.rotation = (((state.rotation + direction * 90) % 360) + 360) % 360;

  // Highlights are computed from unrotated page coordinates, so they would sit
  // in the wrong place on a rotated page. Rather than draw them wrong, they
  // are suppressed while rotated — an honest gap instead of a misleading one.
  invalidate(false);

  if (state.zoomMode !== "manual") void applyFit();
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

    await loadDocument(selected, undefined);
  } catch (error) {
    showError(`Could not open document: ${String(error)}`);
  }
}

/**
 * Opens a path, prompting for a password as many times as the user is willing.
 *
 * Retries rather than giving up after one wrong password: mistyping is the
 * common case, and making the user reopen the file for it is needless.
 */
async function loadDocument(path: string, password: string | undefined): Promise<void> {
  let attempt = password;
  let message = "This document is password protected.";

  for (;;) {
    try {
      const info = await invoke<DocumentInfo>("open_document", {
        path,
        password: attempt ?? null,
      });

      state.document = info;
      state.page = 0;
      state.zoom = 1;
      state.zoomMode = "manual";
      state.rotation = 0;
      state.pageSizes.clear();
      state.layouts.clear();
      state.selection = { page: -1, from: -1, to: -1, dragging: false };
      state.search = { query: "", hits: [], current: -1, truncated: false };
      element<HTMLInputElement>("search-input").value = "";

      clearError();
      updateChrome();
      invalidate(true);
      await applyFit("fit-page");
      await loadOutline();
      return;
    } catch (error) {
      const open = error as Partial<OpenError>;
      if (open?.needsPassword !== true) {
        showError(`Could not open document: ${open?.message ?? String(error)}`);
        return;
      }

      const entered = await askForPassword(message);
      if (entered === null) return; // cancelled; not an error
      attempt = entered;
      message = "That password was not accepted. Try again.";
    }
  }
}

// --- outline ---------------------------------------------------------------

async function loadOutline(): Promise<void> {
  const list = element("outline-list");
  const empty = element("outline-empty");

  try {
    state.outline = await invoke<OutlineEntry[]>("outline");
  } catch {
    state.outline = [];
  }

  list.replaceChildren();
  empty.hidden = state.outline.length > 0;

  for (const entry of state.outline) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = entry.title || "(untitled)";
    // Indentation carries the nesting the flattened list would otherwise lose.
    button.style.paddingLeft = `${0.4 + entry.depth * 0.75}rem`;

    if (entry.page === null) {
      // Shown but not clickable: an entry whose action is not a page jump
      // should not silently send the user to page one.
      button.disabled = true;
      button.title = "This entry does not link to a page";
    } else {
      const target = entry.page;
      button.addEventListener("click", () => goToPage(target));
    }

    item.append(button);
    list.append(item);
  }
}

/** Selects everything on the current page. */
async function selectWholePage(): Promise<void> {
  if (state.document === null || state.rotation !== 0) return;

  try {
    const layout = await textLayout(state.page);
    if (layout.chars.length === 0) return;
    state.selection = {
      page: state.page,
      from: 0,
      to: layout.chars.length - 1,
      dragging: false,
    };
    drawHighlights(state.page);
  } catch (error) {
    showError(`Could not select page text: ${String(error)}`);
  }
}

function toggleOutline(): void {
  const panel = element("outline");
  panel.hidden = !panel.hidden;
  element("toggle-outline").classList.toggle("active", !panel.hidden);
}

// --- password --------------------------------------------------------------

/**
 * Asks for a document password.
 *
 * Resolves to the password, or null if the user cancelled. The value is used
 * once and handed straight to the backend; it is never stored or logged.
 */
function askForPassword(message: string): Promise<string | null> {
  const dialog = element<HTMLDialogElement>("password-dialog");
  const input = element<HTMLInputElement>("password-input");
  const submit = element("password-submit");
  const cancel = element("password-cancel");

  element("password-message").textContent = message;
  input.value = "";

  return new Promise((resolve) => {
    let settled = false;

    /**
     * Settles from the button handlers, not from the dialog's `close` event.
     *
     * Two engine behaviours made the event-driven version hang: `<form
     * method="dialog">` had not set `returnValue` by the time the handler ran,
     * and `dialog.close()` closed the dialog **without dispatching `close` at
     * all**. The promise then never settled, so the retry loop waited forever
     * — no dialog, no error, nothing. Resolving where the user's intent is
     * actually known removes the dependency entirely.
     */
    const settle = (value: string | null): void => {
      if (settled) return;
      settled = true;

      submit.removeEventListener("click", onSubmit);
      cancel.removeEventListener("click", onCancel);
      input.removeEventListener("keydown", onKey);
      dialog.removeEventListener("close", onDismiss);

      // Never leave a password sitting in the DOM.
      input.value = "";
      resolve(value);
    };

    function onSubmit(): void {
      const entered = input.value;
      settle(entered);
      dialog.close();
    }
    function onCancel(): void {
      settle(null);
      dialog.close();
    }
    function onKey(event: KeyboardEvent): void {
      if (event.key === "Enter") {
        event.preventDefault();
        onSubmit();
      }
    }
    /** Escape dismisses natively, with no click to observe. */
    function onDismiss(): void {
      settle(null);
    }

    submit.addEventListener("click", onSubmit);
    cancel.addEventListener("click", onCancel);
    input.addEventListener("keydown", onKey);
    dialog.addEventListener("close", onDismiss);

    dialog.showModal();
    input.focus();
  });
}

async function showSandboxStatus(): Promise<void> {
  const badge = element("sandbox-badge");
  try {
    const worker = await invoke<WorkerStatus>("worker_status");

    // A worker with no engine can only refuse documents. That is a packaging
    // fault and the user should hear about it before picking a file, not
    // after — so it takes precedence over the confinement badge.
    if (worker.running && !worker.engineAvailable) {
      badge.textContent = "no PDF engine";
      badge.title = "The PDF engine is missing from this build.";
      badge.classList.add("warn");
      showError("The PDF engine is missing from this build; documents cannot be opened.");
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
  element("rotate-left").addEventListener("click", () => rotate(-1));
  element("rotate-right").addEventListener("click", () => rotate(1));
  element("toggle-outline").addEventListener("click", () => toggleOutline());
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

    if ((event.metaKey || event.ctrlKey) && event.key === "c") {
      if (selectedText() !== "") {
        event.preventDefault();
        void copySelection();
      }
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key === "a") {
      event.preventDefault();
      void selectWholePage();
      return;
    }

    if (event.key === "Escape") {
      clearSelection();
      return;
    }

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

// Page slots, rendering, and the overlay canvas.
//
// Owns the pixels and the layout; knows nothing about search, selection or any
// other feature — those register layers and tools instead.

import { commands, type PageDimensions } from "../ipc.js";
import { notify, state } from "../state.js";
import { activeLayers, anyContent, makeContext } from "./layers.js";
import { dispatch } from "./tools.js";

/** Scale of the instant first paint, before the sharp render arrives. */
const PREVIEW_SCALE = 0.25;

/** Padding around a page inside the viewport, in CSS pixels. */
export const VIEWPORT_PADDING = 48;

/**
 * How many pages either side of the viewport keep their pixels.
 *
 * The whole memory story in scroll mode. Two covers a normal scroll gesture
 * without holding a document's worth of bitmaps.
 */
const RETAINED_PAGE_RADIUS = 2;

interface Slot {
  readonly root: HTMLElement;
  readonly canvas: HTMLCanvasElement;
  readonly overlay: HTMLCanvasElement;
  /** Identifies what is painted, so identical work is skipped. */
  renderedKey: string | null;
  hasPixels: boolean;
}

const slots = new Map<number, Slot>();

let viewport: HTMLElement;
let pagesContainer: HTMLElement;
let onError: (message: string) => void = () => {};

export function initViewer(
  viewportElement: HTMLElement,
  pagesElement: HTMLElement,
  errorReporter: (message: string) => void,
): void {
  viewport = viewportElement;
  pagesContainer = pagesElement;
  onError = errorReporter;
}

// --- page geometry ---------------------------------------------------------

/** A page's size in points, fetched once and remembered. */
export async function pageSize(page: number): Promise<PageDimensions> {
  const known = state.pageSizes.get(page);
  if (known !== undefined) return known;

  const size = await commands.pageSize(page);
  state.pageSizes.set(page, size);
  return size;
}

/** Page dimensions as displayed, accounting for rotation. */
export function displayedSize(size: PageDimensions): PageDimensions {
  const quarterTurn = state.rotation === 90 || state.rotation === 270;
  return quarterTurn ? { width: size.height, height: size.width } : size;
}

/**
 * The best size estimate available without waiting.
 *
 * Layout must be stable before pixels arrive, or a page finishing its render
 * shoves the ones below it and the scroll position jumps.
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

  // A fresh <canvas> allocates a 300x150 backing store immediately. Across
  // hundreds of slots that is tens of megabytes held for pages nobody has
  // looked at.
  const canvas = document.createElement("canvas");
  canvas.className = "page-canvas";
  canvas.width = 0;
  canvas.height = 0;

  const overlay = document.createElement("canvas");
  overlay.className = "page-overlay";
  overlay.width = 0;
  overlay.height = 0;

  root.append(canvas, overlay);
  const slot: Slot = { root, canvas, overlay, renderedKey: null, hasPixels: false };

  const pointFrom = (event: PointerEvent) => pointToPageCoords(event, slot, page);

  root.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    root.setPointerCapture(event.pointerId);
    dispatch("down", { page, event, point: pointFrom(event) });
  });
  root.addEventListener("pointermove", (event) => {
    dispatch("move", { page, event, point: pointFrom(event) });
  });
  root.addEventListener("pointerup", (event) => {
    if (root.hasPointerCapture(event.pointerId)) root.releasePointerCapture(event.pointerId);
    dispatch("up", { page, event, point: pointFrom(event) });
  });

  return slot;
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

  return {
    x: ((event.clientX - bounds.left) / bounds.width) * size.width,
    // PDF's origin is bottom-left; the pointer's is top-left.
    y: size.height - ((event.clientY - bounds.top) / bounds.height) * size.height,
  };
}

function layOutSlot(slot: Slot, page: number): void {
  const size = estimatedSize(page);
  slot.root.style.width = `${Math.round(size.width * state.zoom)}px`;
  slot.root.style.height = `${Math.round(size.height * state.zoom)}px`;
}

/** Frees a slot's pixel buffers while leaving its place in the layout. */
function releaseSlotPixels(slot: Slot): void {
  // Setting a dimension to zero is what releases the backing store; hiding or
  // clearing the canvas does not.
  slot.canvas.width = 0;
  slot.canvas.height = 0;
  slot.overlay.width = 0;
  slot.overlay.height = 0;
  slot.renderedKey = null;
  slot.hasPixels = false;
}

function buildSlots(): void {
  const info = state.document;
  pagesContainer.replaceChildren();
  slots.clear();

  if (info === null) return;

  const pages =
    state.viewMode === "scroll"
      ? Array.from({ length: info.pageCount }, (_, index) => index)
      : [state.page];

  for (const page of pages) {
    const slot = createSlot(page);
    layOutSlot(slot, page);
    slots.set(page, slot);
    pagesContainer.append(slot.root);
  }

  pagesContainer.hidden = false;
}

// --- rendering -------------------------------------------------------------

function renderKey(page: number): string {
  return `${page}@${(state.zoom * window.devicePixelRatio).toFixed(3)}r${state.rotation}`;
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
 * during a fast scroll only builds a queue of work that is stale by the time
 * it runs. `.catch` before `.then` matters: a rejected chain skips every
 * subsequent `.then` forever, which would silently stop all rendering.
 */
let renderChain: Promise<void> = Promise.resolve();

function scheduleRender(page: number): void {
  const generation = state.generation;
  renderChain = renderChain.catch(() => undefined).then(async () => {
    if (generation !== state.generation) return;
    // Re-checked on the way out of the queue, not just on the way in.
    if (!isPageWanted(page)) return;
    await renderSlot(page, generation);
  });
}

function isPageWanted(page: number): boolean {
  if (state.viewMode === "page") return page === state.page;
  const { first, last } = visiblePageRange();
  return page >= first - RETAINED_PAGE_RADIUS && page <= last + RETAINED_PAGE_RADIUS;
}

async function renderSlot(page: number, generation: number): Promise<void> {
  const slot = slots.get(page);
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
    // pixels, dropping to a low-resolution version first is a visible
    // downgrade rather than an improvement.
    if (!slot.hasPixels) {
      const preview = await commands.renderPage(
        page,
        Math.max(deviceZoom * PREVIEW_SCALE, 0.05),
        state.rotation,
      );
      if (generation !== state.generation) return;
      paintSlot(slot, decodePage(preview), cssWidth, cssHeight);
    }

    const full = await commands.renderPage(page, deviceZoom, state.rotation);
    if (generation !== state.generation) return;

    paintSlot(slot, decodePage(full), cssWidth, cssHeight);
    slot.renderedKey = key;
    drawOverlay(page);
  } catch (error) {
    if (generation === state.generation) {
      onError(`Could not render page ${page + 1}: ${String(error)}`);
    }
  }
}

/** Renders what is on screen and releases what is not. */
export function updateVisiblePages(): void {
  if (state.document === null) return;

  if (state.viewMode === "page") {
    scheduleRender(state.page);
    return;
  }

  const visible = visiblePageRange();
  for (const [page, slot] of slots) {
    const near =
      page >= visible.first - RETAINED_PAGE_RADIUS &&
      page <= visible.last + RETAINED_PAGE_RADIUS;

    if (near) scheduleRender(page);
    else if (slot.hasPixels) releaseSlotPixels(slot);
  }

  if (visible.first !== state.page) {
    state.page = visible.first;
    notify("page");
  }
}

function visiblePageRange(): { first: number; last: number } {
  const top = viewport.scrollTop;
  const bottom = top + viewport.clientHeight;

  let first = 0;
  let last = 0;
  let found = false;

  for (const [page, slot] of slots) {
    const slotTop = slot.root.offsetTop;
    if (slotTop + slot.root.offsetHeight >= top && slotTop <= bottom) {
      if (!found) {
        first = page;
        found = true;
      }
      last = page;
    }
  }

  return found ? { first, last } : { first: 0, last: 0 };
}

// --- overlay ---------------------------------------------------------------

/**
 * Draws every registered layer over one page.
 *
 * The overlay canvas is sized to zero when no layer has anything to draw: a
 * full-size second canvas per page doubles the memory of every rendered page,
 * and most of the time nothing is highlighted or selected.
 */
export function drawOverlay(page: number): void {
  const slot = slots.get(page);
  if (slot === undefined) return;

  const size = state.pageSizes.get(page);

  // Layer coordinates are unrotated page points. Mapping them through a display
  // rotation is straightforward but untested, and drawing in the wrong place is
  // worse than not drawing — so nothing is drawn while rotated.
  const rotated = state.rotation !== 0;

  if (rotated || size === undefined || size.width <= 0 || !anyContent(page)) {
    slot.overlay.width = 0;
    slot.overlay.height = 0;
    return;
  }

  slot.overlay.width = slot.canvas.width;
  slot.overlay.height = slot.canvas.height;
  slot.overlay.style.width = slot.canvas.style.width;
  slot.overlay.style.height = slot.canvas.style.height;

  const context = slot.overlay.getContext("2d");
  if (context === null) return;
  context.clearRect(0, 0, slot.overlay.width, slot.overlay.height);

  const layerContext = makeContext(
    page,
    context,
    size,
    slot.overlay.width,
    slot.overlay.height,
  );

  for (const layer of activeLayers(page)) {
    context.save();
    try {
      layer.draw(layerContext);
    } catch (error) {
      // One broken layer must not blank the others.
      console.error(`layer "${layer.name}" failed:`, error);
    }
    context.restore();
  }
}

export function redrawAllOverlays(): void {
  for (const page of slots.keys()) drawOverlay(page);
}

// --- view control ----------------------------------------------------------

/** Discards in-flight work and rebuilds the view. */
export function invalidate(rebuild: boolean): void {
  state.generation += 1;

  if (rebuild) {
    buildSlots();
  } else {
    for (const [page, slot] of slots) {
      slot.renderedKey = null;
      layOutSlot(slot, page);
    }
  }

  // Force layout before deciding what is visible. Measuring straight after
  // building slots reads every offset as zero, so every page looks on-screen.
  // requestAnimationFrame is wrong here: it does not fire while a window is
  // minimised, so a document opened in the background would stay blank.
  void pagesContainer.offsetHeight;
  updateVisiblePages();
}

export function scrollToPage(page: number): void {
  const slot = slots.get(page);
  if (slot === undefined) return;
  viewport.scrollTo({ top: slot.root.offsetTop - 16, behavior: "auto" });
}

export function slotPages(): number[] {
  return [...slots.keys()];
}

export function viewportSize(): { width: number; height: number } {
  return { width: viewport.clientWidth, height: viewport.clientHeight };
}

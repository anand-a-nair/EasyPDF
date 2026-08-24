// Frontend entry point.
//
// No framework, deliberately — see ideas/03-tech-decisions.md (TD-006). The UI
// is a canvas, a toolbar, and a few dialogs; a framework would cost startup
// time and bundle size against the budget for very little benefit.

import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

interface DocumentInfo {
  readonly name: string;
  readonly pageCount: number;
  readonly encrypted: boolean;
}

interface WorkerStatus {
  readonly running: boolean;
  readonly sandboxed: boolean;
  readonly detail: string;
  readonly memoryCapped: boolean;
}

/** Zoom stops, in the order the +/- buttons step through. */
const ZOOM_STOPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4] as const;

/** Scale used for the instant first paint. See renderPage. */
const PREVIEW_SCALE = 0.25;

const state = {
  document: null as DocumentInfo | null,
  page: 0,
  zoom: 1,
  /** Guards against an older render finishing after a newer one. */
  renderToken: 0,
};

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) throw new Error(`missing element: ${id}`);
  return found as T;
}

const canvas = element<HTMLCanvasElement>("page");
const viewport = element("viewport");
const errorBar = element("error");

function showError(message: string): void {
  errorBar.textContent = message;
  errorBar.hidden = false;
}

function clearError(): void {
  errorBar.hidden = true;
}

/**
 * Decodes the render response: u32 width, u32 height, then RGBA bytes.
 *
 * Raw bytes rather than JSON — a page is hundreds of kilobytes, and base64
 * would inflate it by a third and cost a parse on both sides.
 */
function decodePage(buffer: ArrayBuffer): ImageData {
  const header = new DataView(buffer, 0, 8);
  const width = header.getUint32(0, true);
  const height = header.getUint32(4, true);
  const pixels = new Uint8ClampedArray(buffer, 8);

  if (pixels.length !== width * height * 4) {
    // The host validates worker responses, but the shell should not trust a
    // malformed frame either.
    throw new Error(`page data does not match its ${width}x${height} header`);
  }

  return new ImageData(pixels, width, height);
}

/** Paints an ImageData onto the canvas at the given CSS size. */
function paint(image: ImageData, cssWidth: number, cssHeight: number): void {
  canvas.width = image.width;
  canvas.height = image.height;
  canvas.style.width = `${cssWidth}px`;
  canvas.style.height = `${cssHeight}px`;

  const context = canvas.getContext("2d");
  if (context === null) throw new Error("could not get a 2d canvas context");
  context.putImageData(image, 0, 0);
  canvas.hidden = false;
}

/**
 * Renders the current page.
 *
 * Two passes: a cheap low-resolution paint first, then the sharp one. Users
 * read perceived latency, not actual latency — a blurry page immediately beats
 * a crisp page in 300ms. See ideas/06-features.md.
 */
async function renderPage(): Promise<void> {
  if (state.document === null) return;

  const token = ++state.renderToken;
  const { page } = state;

  // Backing store is in device pixels so pages stay sharp on retina displays.
  const deviceZoom = state.zoom * window.devicePixelRatio;

  canvas.classList.add("loading");

  try {
    // Pass 1: instant, blurry.
    const preview = await invoke<ArrayBuffer>("render_page", {
      page,
      zoom: Math.max(deviceZoom * PREVIEW_SCALE, 0.05),
    });
    if (token !== state.renderToken) return;

    const previewImage = decodePage(preview);
    const cssWidth = previewImage.width / (window.devicePixelRatio * PREVIEW_SCALE);
    const cssHeight = previewImage.height / (window.devicePixelRatio * PREVIEW_SCALE);
    paint(previewImage, cssWidth, cssHeight);

    // Pass 2: the real thing.
    const full = await invoke<ArrayBuffer>("render_page", { page, zoom: deviceZoom });
    if (token !== state.renderToken) return;

    const fullImage = decodePage(full);
    paint(
      fullImage,
      fullImage.width / window.devicePixelRatio,
      fullImage.height / window.devicePixelRatio,
    );
    clearError();
  } catch (error) {
    if (token === state.renderToken) showError(`Could not render page: ${String(error)}`);
  } finally {
    if (token === state.renderToken) canvas.classList.remove("loading");
  }
}

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
}

async function goToPage(page: number): Promise<void> {
  const info = state.document;
  if (info === null) return;

  const clamped = Math.max(0, Math.min(page, info.pageCount - 1));
  if (clamped === state.page) return;

  state.page = clamped;
  updateChrome();
  await renderPage();
}

async function setZoom(zoom: number): Promise<void> {
  state.zoom = Math.max(0.1, Math.min(zoom, 8));
  updateChrome();
  await renderPage();
}

function stepZoom(direction: 1 | -1): Promise<void> {
  const current = state.zoom;
  const stops = direction === 1 ? ZOOM_STOPS : [...ZOOM_STOPS].reverse();
  const next = stops.find((stop) =>
    direction === 1 ? stop > current + 0.001 : stop < current - 0.001,
  );
  return setZoom(next ?? current);
}

/** Scales the page so its full height fits the viewport. */
async function zoomToFit(): Promise<void> {
  if (state.document === null) return;

  // Measure at a known zoom, then solve for the scale that fits.
  const probe = await invoke<ArrayBuffer>("render_page", { page: state.page, zoom: 1 });
  const image = decodePage(probe);

  const available = viewport.clientHeight - 64; // padding
  await setZoom(Math.max(0.1, Math.min(available / image.height, 4)));
}

async function openDocument(): Promise<void> {
  const selected = await openFileDialog({
    multiple: false,
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (typeof selected !== "string") return;

  try {
    const info = await invoke<DocumentInfo>("open_document", { path: selected });
    state.document = info;
    state.page = 0;
    state.zoom = 1;
    clearError();
    updateChrome();
    await renderPage();
  } catch (error) {
    showError(`Could not open document: ${String(error)}`);
  }
}

/**
 * Shows how well the worker confined itself.
 *
 * Surfaced rather than assumed: a worker running with ordinary user privileges
 * is something the user deserves to know about. See ideas/07-security.md.
 */
async function showSandboxStatus(): Promise<void> {
  const badge = element("sandbox-badge");
  try {
    const worker = await invoke<WorkerStatus>("worker_status");
    if (worker.sandboxed) {
      badge.textContent = "sandboxed";
      badge.title = worker.detail;
      badge.classList.remove("warn");
    } else {
      badge.textContent = "NOT sandboxed";
      badge.title = worker.detail;
      badge.classList.add("warn");
    }
  } catch {
    badge.textContent = "worker unavailable";
    badge.classList.add("warn");
  }
}

function wireUp(): void {
  element("open").addEventListener("click", () => void openDocument());
  element("open-empty").addEventListener("click", () => void openDocument());
  element("prev").addEventListener("click", () => void goToPage(state.page - 1));
  element("next").addEventListener("click", () => void goToPage(state.page + 1));
  element("zoom-in").addEventListener("click", () => void stepZoom(1));
  element("zoom-out").addEventListener("click", () => void stepZoom(-1));
  element("zoom-fit").addEventListener("click", () => void zoomToFit());

  document.addEventListener("keydown", (event) => {
    if (state.document === null) return;
    switch (event.key) {
      case "ArrowRight":
      case "PageDown":
        void goToPage(state.page + 1);
        break;
      case "ArrowLeft":
      case "PageUp":
        void goToPage(state.page - 1);
        break;
      case "Home":
        void goToPage(0);
        break;
      case "End":
        void goToPage(state.document.pageCount - 1);
        break;
      default:
        break;
    }
  });
}

wireUp();
updateChrome();
void showSandboxStatus();

// Page navigation, zoom, rotation, and view mode.

import { notify, state, type ViewMode, type ZoomMode } from "../state.js";
import { displayedSize, invalidate, pageSize, scrollToPage, VIEWPORT_PADDING, viewportSize }
  from "../viewer/viewer.js";

const ZOOM_STOPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4] as const;
const ZOOM_STEP_FACTOR = 1.25;
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 8;

export function goToPage(page: number): void {
  const info = state.document;
  if (info === null) return;

  const clamped = Math.max(0, Math.min(page, info.pageCount - 1));
  if (clamped === state.page && state.viewMode === "page") return;

  state.page = clamped;
  notify("page");

  if (state.viewMode === "scroll") {
    scrollToPage(clamped);
  } else {
    invalidate(true);
  }
}

export function setZoom(zoom: number, mode: ZoomMode): void {
  state.zoom = Math.max(MIN_ZOOM, Math.min(zoom, MAX_ZOOM));
  state.zoomMode = mode;
  notify("view");
  invalidate(false);
}

export function stepZoom(direction: 1 | -1): void {
  const current = state.zoom;
  const stops = direction === 1 ? [...ZOOM_STOPS] : [...ZOOM_STOPS].reverse();
  const next = stops.find((stop) =>
    direction === 1 ? stop > current + 0.001 : stop < current - 0.001,
  );

  // Fitting a small page can land above the largest stop, leaving no next stop
  // to step to — zoom in would silently do nothing.
  const target =
    next ?? (direction === 1 ? current * ZOOM_STEP_FACTOR : current / ZOOM_STEP_FACTOR);

  // A manual zoom cancels fit: the user has taken over.
  setZoom(target, "manual");
}

/**
 * Applies the current fit mode.
 *
 * Called on resize as well as on demand, which is what makes fit *stay* fitted
 * when the window changes.
 */
export async function applyFit(mode: ZoomMode = state.zoomMode): Promise<void> {
  if (state.document === null || mode === "manual") return;

  const viewport = viewportSize();
  const available = {
    width: viewport.width - VIEWPORT_PADDING,
    height: viewport.height - VIEWPORT_PADDING,
  };

  // Before first layout, or in a window too small to mean anything, fitting
  // would collapse to the minimum zoom. Keeping the current zoom is less
  // surprising.
  if (available.width < 50 || available.height < 50) return;

  const size = displayedSize(await pageSize(state.page));
  if (size.width <= 0 || size.height <= 0) return;

  const scale =
    mode === "fit-width"
      ? available.width / size.width
      : Math.min(available.width / size.width, available.height / size.height);

  setZoom(scale, mode);
}

export function rotate(direction: 1 | -1): void {
  state.rotation = (((state.rotation + direction * 90) % 360) + 360) % 360;
  notify("view");
  invalidate(false);
  if (state.zoomMode !== "manual") void applyFit();
}

export function setViewMode(mode: ViewMode): void {
  if (state.viewMode === mode) return;
  state.viewMode = mode;
  notify("view");
  invalidate(true);
  if (mode === "scroll") scrollToPage(state.page);
}

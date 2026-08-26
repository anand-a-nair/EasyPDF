// Text selection: an overlay layer plus a pointer tool.

import { commands, type TextLayout, type TextRect } from "../ipc.js";
import { notify, state } from "../state.js";
import { registerLayer } from "../viewer/layers.js";
import { registerTool } from "../viewer/tools.js";
import { drawOverlay, redrawAllOverlays } from "../viewer/viewer.js";

const SELECTION_FILL = "rgba(64, 120, 220, 0.35)";

/** Fetches a page's character geometry once, then reuses it. */
async function textLayout(page: number): Promise<TextLayout> {
  const known = state.layouts.get(page);
  if (known !== undefined) return known;

  const layout = await commands.textLayout(page);
  state.layouts.set(page, layout);
  return layout;
}

function selectedRects(page: number): TextRect[] {
  const { page: selectedPage, from, to } = state.selection;
  if (selectedPage !== page || from < 0 || to < 0) return [];

  const layout = state.layouts.get(page);
  if (layout === undefined) return [];

  const [start, end] = from <= to ? [from, to] : [to, from];
  return layout.chars.slice(start, end + 1).map((character) => character.rect);
}

export function selectedText(): string {
  const { page, from, to } = state.selection;
  const layout = state.layouts.get(page);
  if (layout === undefined || from < 0 || to < 0) return "";

  const [start, end] = from <= to ? [from, to] : [to, from];
  return layout.chars.slice(start, end + 1).map((character) => character.text).join("");
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

export function clearSelection(): void {
  const previous = state.selection.page;
  state.selection = { page: -1, from: -1, to: -1, dragging: false };
  notify("selection");
  if (previous >= 0) drawOverlay(previous);
}

export async function selectWholePage(onError: (message: string) => void): Promise<void> {
  if (state.document === null || state.rotation !== 0) return;

  try {
    const layout = await textLayout(state.page);
    if (layout.chars.length === 0) return;
    state.selection = { page: state.page, from: 0, to: layout.chars.length - 1, dragging: false };
    notify("selection");
    drawOverlay(state.page);
  } catch (error) {
    onError(`Could not select page text: ${String(error)}`);
  }
}

export function registerSelection(onError: (message: string) => void): void {
  registerLayer({
    name: "selection",
    // Above search, so a selected hit reads as selected.
    order: 20,
    hasContent: (page) => selectedRects(page).length > 0,
    draw: ({ page, context, toCanvas }) => {
      context.fillStyle = SELECTION_FILL;
      for (const rect of selectedRects(page)) {
        const box = toCanvas(rect);
        if (box.width <= 0 || box.height <= 0) continue;
        context.fillRect(box.x, box.y, box.width, box.height);
      }
    },
  });

  registerTool({
    name: "select",
    cursor: "text",

    onPointerDown: ({ page, point }) => {
      if (point === null || state.rotation !== 0) return;

      void (async () => {
        try {
          const layout = await textLayout(page);
          const index = nearestCharIndex(layout, point.x, point.y);
          if (index < 0) return;

          const previous = state.selection.page;
          state.selection = { page, from: index, to: index, dragging: true };
          notify("selection");

          if (previous >= 0 && previous !== page) drawOverlay(previous);
          drawOverlay(page);
        } catch (error) {
          onError(`Could not prepare text selection: ${String(error)}`);
        }
      })();
    },

    onPointerMove: ({ page, point }) => {
      if (!state.selection.dragging || state.selection.page !== page || point === null) return;

      const layout = state.layouts.get(page);
      if (layout === undefined) return;

      const index = nearestCharIndex(layout, point.x, point.y);
      if (index < 0 || index === state.selection.to) return;

      state.selection.to = index;
      notify("selection");
      drawOverlay(page);
    },

    onPointerUp: () => {
      state.selection.dragging = false;
    },

    onDeactivate: () => clearSelection(),
  });
}

/**
 * Copies the selection.
 *
 * Tries the async clipboard API, then falls back to a hidden textarea. The
 * webview is not always a secure context, and `navigator.clipboard` is simply
 * absent when it is not — a silent no-op on Copy would be worse than the
 * deprecated fallback.
 */
export async function copySelection(onError: (message: string) => void): Promise<void> {
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
    onError("Could not copy the selection.");
  } finally {
    scratch.remove();
  }
}

export { redrawAllOverlays };

// Full-document search, registered as an overlay layer.

import { commands } from "../ipc.js";
import { notify, state } from "../state.js";
import { registerLayer } from "../viewer/layers.js";
import { drawOverlay, redrawAllOverlays } from "../viewer/viewer.js";
import { goToPage } from "./navigation.js";

const CURRENT_HIT = "rgba(255, 145, 0, 0.45)";
const OTHER_HIT = "rgba(255, 213, 0, 0.35)";

export function registerSearchLayer(): void {
  registerLayer({
    name: "search",
    // Below selection: a selected search hit should read as selected.
    order: 10,
    hasContent: (page) => state.search.hits.some((hit) => hit.page === page),
    draw: ({ page, context, toCanvas }) => {
      state.search.hits.forEach((hit, index) => {
        if (hit.page !== page) return;
        context.fillStyle = index === state.search.current ? CURRENT_HIT : OTHER_HIT;

        for (const rect of hit.rects) {
          const box = toCanvas(rect);
          if (box.width <= 0 || box.height <= 0) continue;
          context.fillRect(box.x, box.y, box.width, box.height);
        }
      });
    },
  });
}

export async function runSearch(query: string, onError: (message: string) => void): Promise<void> {
  state.search.query = query;

  if (query === "" || state.document === null) {
    state.search.hits = [];
    state.search.current = -1;
    state.search.truncated = false;
    notify("search");
    redrawAllOverlays();
    return;
  }

  try {
    // Case-insensitive by default: it is what people expect from a find box.
    const results = await commands.search(query, false);
    state.search.hits = results.hits;
    state.search.truncated = results.truncated;
    state.search.current = results.hits.length > 0 ? 0 : -1;
    notify("search");

    if (results.hits.length === 0) {
      // Must redraw explicitly: goToHit returns early with no hits, which
      // would leave the previous search's highlights on screen while the
      // toolbar said "no matches".
      redrawAllOverlays();
      return;
    }

    goToHit(0);
  } catch (error) {
    onError(`Search failed: ${String(error)}`);
  }
}

export function goToHit(index: number): void {
  const { hits } = state.search;
  if (hits.length === 0) return;

  // Wrapping is what every find box does; stopping at the last hit reads as
  // broken.
  const wrapped = ((index % hits.length) + hits.length) % hits.length;
  state.search.current = wrapped;

  const hit = hits[wrapped];
  if (hit === undefined) return;

  notify("search");
  if (hit.page !== state.page) goToPage(hit.page);
  redrawAllOverlays();
  drawOverlay(hit.page);
}

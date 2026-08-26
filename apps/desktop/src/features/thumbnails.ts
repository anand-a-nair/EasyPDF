// Page thumbnails, as a sidebar panel.
//
// Written entirely against the panel API added in this refactor — no changes
// to the viewer, the toolbar, or any layout code. That was the point of the
// exercise: if a new panel had needed edits elsewhere, the seam would not be
// worth having.
//
// Thumbnails are virtualised for the same reason scroll mode is. A 500-page
// document must not hold 500 bitmaps, even small ones, so only thumbnails
// scrolled into view are rendered and each is released when it leaves.
//
// Visibility is measured explicitly rather than observed. `IntersectionObserver`
// is the obvious tool and would be less code, but this project has now been
// bitten three times by observer APIs that silently never fire —
// `requestAnimationFrame` in a minimised window, an unreferenced
// `ResizeObserver`, and `IntersectionObserver` in the test browser. The viewer
// already measures visibility directly for the same reason; doing it the same
// way here means one pattern to understand and one less API to trust.

import { commands } from "../ipc.js";
import { state, subscribe } from "../state.js";
import { registerPanel } from "../ui/panels.js";
import { goToPage } from "./navigation.js";

/** Thumbnail width in CSS pixels. Height follows the page's aspect ratio. */
const THUMBNAIL_WIDTH = 120;

/** How far outside the visible list to keep thumbnails rendered. */
const VISIBILITY_MARGIN_PX = 300;

const SCROLL_DEBOUNCE_MS = 80;

/** Rendered at device resolution so they are not blurry on retina displays. */
function thumbnailZoom(pageWidth: number): number {
  return (THUMBNAIL_WIDTH / pageWidth) * window.devicePixelRatio;
}

export function registerThumbnailPanel(): void {
  let list: HTMLElement;
  const cells = new Map<number, HTMLElement>();
  let builtFor: string | null = null;

  /** Renders one thumbnail, once. */
  async function renderThumbnail(page: number): Promise<void> {
    const cell = cells.get(page);
    if (cell === undefined || cell.dataset["rendered"] !== undefined) return;

    const canvas = cell.querySelector("canvas");
    if (canvas === null) return;

    // Claimed before awaiting, so a second visibility pass during the render
    // does not queue the same page again.
    cell.dataset["rendered"] = "pending";

    try {
      const size = await commands.pageSize(page);
      if (size.width <= 0) return;

      const buffer = await commands.renderPage(page, thumbnailZoom(size.width), state.rotation);

      const header = new DataView(buffer, 0, 8);
      const width = header.getUint32(0, true);
      const height = header.getUint32(4, true);
      const pixels = new Uint8ClampedArray(buffer, 8);
      if (pixels.length !== width * height * 4) return;

      canvas.width = width;
      canvas.height = height;
      canvas.style.width = `${THUMBNAIL_WIDTH}px`;
      canvas.style.height = `${Math.round(height / window.devicePixelRatio)}px`;

      const context = canvas.getContext("2d");
      if (context === null) return;
      context.putImageData(new ImageData(pixels, width, height), 0, 0);
      cell.dataset["rendered"] = "yes";
    } catch {
      // A thumbnail that will not render is not worth an error banner; the
      // page itself reports the problem if the user navigates to it.
      cell.dataset["rendered"] = "failed";
    }
  }

  function releaseThumbnail(page: number): void {
    const cell = cells.get(page);
    const canvas = cell?.querySelector("canvas");
    if (cell === undefined || canvas === null || canvas === undefined) return;

    canvas.width = 0;
    canvas.height = 0;
    delete cell.dataset["rendered"];
  }

  /**
   * The scrolling ancestor.
   *
   * Not the list itself: the list grows to its content and the sidebar is what
   * actually scrolls. Measuring against the list read `scrollTop` as zero and
   * its height as the full content height, so every thumbnail looked visible
   * and a 200-page document rendered all 200.
   */
  function scrollHost(): HTMLElement {
    let node: HTMLElement | null = list.parentElement;
    while (node !== null) {
      const overflow = window.getComputedStyle(node).overflowY;
      if (overflow === "auto" || overflow === "scroll") return node;
      node = node.parentElement;
    }
    return list;
  }

  /** Renders what is on screen and releases what is not. */
  function updateVisibleThumbnails(): void {
    if (cells.size === 0) return;

    // Viewport-relative rectangles rather than offset arithmetic: they are
    // correct regardless of which ancestor happens to be positioned.
    const hostBounds = scrollHost().getBoundingClientRect();
    const top = hostBounds.top - VISIBILITY_MARGIN_PX;
    const bottom = hostBounds.bottom + VISIBILITY_MARGIN_PX;

    for (const [page, cell] of cells) {
      const bounds = cell.getBoundingClientRect();
      const visible = bounds.bottom >= top && bounds.top <= bottom;

      if (visible) void renderThumbnail(page);
      else if (cell.dataset["rendered"] !== undefined) releaseThumbnail(page);
    }
  }

  function markCurrent(): void {
    for (const [page, cell] of cells) {
      cell.classList.toggle("current", page === state.page);
    }
  }

  registerPanel({
    name: "thumbnails",
    title: "Pages",

    mount(container) {
      const heading = document.createElement("h2");
      heading.textContent = "Pages";

      list = document.createElement("div");
      list.className = "thumbnail-list";

      container.append(heading, list);

      // Listen on the element that actually scrolls, which is the sidebar.
      let scrollTimer: number | undefined;
      const onScroll = (): void => {
        window.clearTimeout(scrollTimer);
        scrollTimer = window.setTimeout(updateVisibleThumbnails, SCROLL_DEBOUNCE_MS);
      };
      scrollHost().addEventListener("scroll", onScroll, { passive: true });
      list.addEventListener("scroll", onScroll, { passive: true });

      subscribe((change) => {
        if (change === "page") markCurrent();
      });
    },

    refresh() {
      const info = state.document;
      if (info === null) {
        list.replaceChildren();
        cells.clear();
        builtFor = null;
        return;
      }

      // Rebuilt only when the document or its length changes; a refresh from
      // simply reopening the panel must not throw away rendered thumbnails.
      const signature = `${info.name}:${info.pageCount}`;
      if (builtFor !== signature) {
        list.replaceChildren();
        cells.clear();

        for (let page = 0; page < info.pageCount; page += 1) {
          const cell = document.createElement("button");
          cell.type = "button";
          cell.className = "thumbnail";
          cell.dataset["page"] = String(page);

          const canvas = document.createElement("canvas");
          // Zero until rendered: an untouched canvas still allocates 300x150.
          canvas.width = 0;
          canvas.height = 0;

          const label = document.createElement("span");
          label.textContent = String(page + 1);

          cell.append(canvas, label);
          cell.addEventListener("click", () => goToPage(page));
          list.append(cell);
          cells.set(page, cell);
        }

        builtFor = signature;
      }

      markCurrent();

      // Force layout before measuring: straight after building the cells every
      // offset reads as zero, so every thumbnail would look visible.
      void list.offsetHeight;
      updateVisibleThumbnails();
    },
  });
}

// Overlay layers drawn on top of a page.
//
// Search highlights and text selection were drawn by one function with both
// cases hard-coded into it. Every Phase 2 feature that draws on a page —
// annotations, ink, shapes, form field outlines, redaction boxes — would have
// meant editing that function again, and each addition would have had to know
// about all the others.
//
// A layer is just a draw callback plus a predicate for whether it has anything
// to draw. Features register one and never touch this file again.
//
// See ideas/13-frontend-architecture.md.

import type { PageDimensions } from "../ipc.js";

/** What a layer is given: page space in points, canvas space in device pixels. */
export interface LayerContext {
  readonly page: number;
  readonly context: CanvasRenderingContext2D;
  /** Page size in points, unrotated. */
  readonly size: PageDimensions;
  /** Multiply page points by these to get canvas pixels. */
  readonly scaleX: number;
  readonly scaleY: number;
  /**
   * Converts a rectangle in PDF page coordinates to canvas coordinates.
   *
   * PDF's origin is bottom-left in points; a canvas is top-left in pixels.
   * Every layer needs this flip, and every layer getting it wrong
   * independently is how highlights end up on the opposite side of the page.
   */
  readonly toCanvas: (rect: {
    left: number;
    bottom: number;
    right: number;
    top: number;
  }) => { x: number; y: number; width: number; height: number };
}

export interface Layer {
  /** Identifier, for replacing or removing a layer. */
  readonly name: string;
  /**
   * Draw order. Lower draws first, so higher numbers sit on top. Search
   * highlights are below selection, which is below annotations.
   */
  readonly order: number;
  /** Whether this layer has anything to draw on this page. */
  hasContent(page: number): boolean;
  draw(context: LayerContext): void;
}

const layers = new Map<string, Layer>();

export function registerLayer(layer: Layer): void {
  layers.set(layer.name, layer);
}

export function unregisterLayer(name: string): void {
  layers.delete(name);
}

/** Layers with something to draw on this page, in draw order. */
export function activeLayers(page: number): Layer[] {
  return [...layers.values()]
    .filter((layer) => layer.hasContent(page))
    .sort((a, b) => a.order - b.order);
}

/** Whether any layer wants to draw on this page. */
export function anyContent(page: number): boolean {
  return [...layers.values()].some((layer) => layer.hasContent(page));
}

/** Builds the context handed to each layer. */
export function makeContext(
  page: number,
  context: CanvasRenderingContext2D,
  size: PageDimensions,
  canvasWidth: number,
  canvasHeight: number,
): LayerContext {
  const scaleX = canvasWidth / size.width;
  const scaleY = canvasHeight / size.height;

  return {
    page,
    context,
    size,
    scaleX,
    scaleY,
    toCanvas: (rect) => ({
      x: rect.left * scaleX,
      y: (size.height - rect.top) * scaleY,
      width: (rect.right - rect.left) * scaleX,
      height: (rect.top - rect.bottom) * scaleY,
    }),
  };
}

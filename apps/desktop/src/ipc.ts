// Typed wrappers over the Tauri commands.
//
// One place where every payload shape is written down, so a backend change
// surfaces as a type error here rather than as `undefined` somewhere in a
// render path. The shapes are also checked against the real Rust types by
// `scripts/check-contracts.mjs`.

import { invoke } from "@tauri-apps/api/core";

export interface DocumentInfo {
  readonly name: string;
  readonly pageCount: number;
  readonly encrypted: boolean;
}

export interface PageDimensions {
  readonly width: number;
  readonly height: number;
}

export interface TextRect {
  readonly left: number;
  readonly bottom: number;
  readonly right: number;
  readonly top: number;
}

export interface SearchHit {
  readonly page: number;
  readonly rects: readonly TextRect[];
}

export interface SearchResults {
  readonly hits: readonly SearchHit[];
  readonly truncated: boolean;
}

export interface CharBox {
  readonly text: string;
  readonly rect: TextRect;
}

export interface TextLayout {
  readonly chars: readonly CharBox[];
  readonly truncated: boolean;
}

export interface OutlineEntry {
  readonly title: string;
  readonly depth: number;
  readonly page: number | null;
}

export interface WorkerStatus {
  readonly running: boolean;
  readonly sandboxed: boolean;
  readonly detail: string;
  readonly memoryCapped: boolean;
  readonly engineAvailable: boolean;
}

/** The shape `open_document` rejects with. */
export interface OpenError {
  readonly needsPassword: boolean;
  readonly message: string;
}

export const commands = {
  openDocument: (path: string, password: string | null): Promise<DocumentInfo> =>
    invoke("open_document", { path, password }),

  closeDocument: (): Promise<void> => invoke("close_document"),

  pageSize: (page: number): Promise<PageDimensions> => invoke("page_size", { page }),

  /** Raw bytes: `u32` width, `u32` height, then RGBA. */
  renderPage: (page: number, zoom: number, rotation: number): Promise<ArrayBuffer> =>
    invoke("render_page", { page, zoom, rotation }),

  search: (query: string, matchCase: boolean): Promise<SearchResults> =>
    invoke("search", { query, matchCase }),

  textLayout: (page: number): Promise<TextLayout> => invoke("text_layout", { page }),

  outline: (): Promise<OutlineEntry[]> => invoke("outline"),

  workerStatus: (): Promise<WorkerStatus> => invoke("worker_status"),
} as const;

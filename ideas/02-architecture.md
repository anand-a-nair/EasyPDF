# Architecture

## Shape

```
┌─────────────────────────────────────────────────┐
│  UI  (TypeScript, system WebView via Tauri)     │
│  canvas rendering · toolbars · dialogs          │
└────────────────────┬────────────────────────────┘
                     │ Tauri IPC (typed commands)
┌────────────────────┴────────────────────────────┐
│  apps/desktop — Tauri host                      │
│  window/menu/file dialogs · command handlers    │
└────────────────────┬────────────────────────────┘
                     │
   ┌─────────────────┼──────────────────┐
   │                 │                  │
┌──┴───────────┐ ┌───┴──────────┐ ┌─────┴────────┐
│ easypdf-core │ │easypdf-render│ │easypdf-crypto│
│ document     │ │ PDFium FFI   │ │ encryption   │
│ model, page  │ │ rasterize,   │ │ PAdES sign   │
│ ops, save    │ │ tile cache   │ │ cert handling│
└──────┬───────┘ └───┬──────────┘ └─────┬────────┘
       └─────────────┴──────────────────┘
              ┌──────┴───────┐
              │ easypdf-ffi  │  sandboxed parse worker
              └──────────────┘
```

## Crates

**`easypdf-core`** — the document model. Owns parsing to an object graph, page
tree manipulation (reorder, rotate, delete, insert, merge, split), incremental
save, and the undo stack. Zero UI knowledge. This is the crate that must never
corrupt a file, and it carries the heaviest test burden.

**`easypdf-render`** — rasterization. Wraps PDFium through `pdfium-render`,
adds a tile cache keyed by (page, zoom, rotation), and handles progressive
render so a page appears at low resolution immediately and sharpens. Also owns
text extraction and hit-testing for selection.

**`easypdf-crypto`** — standard security handler (RC4 40/128, AES-128/256),
password verification, permission flags, and digital signatures (PKCS#7
detached, PAdES B-B/B-T profiles). Isolated because it needs the most careful
review and the fewest changes.

**`easypdf-ffi`** — the untrusted boundary. See [07-security.md](07-security.md);
parsing runs in a separate low-privilege process, and this crate defines the
message protocol across that line.

**`apps/desktop`** — the Tauri shell. Thin: window management, native menus,
file dialogs, and a typed command layer that forwards to the crates. Business
logic living here is a code smell.

## Process model

Two processes, deliberately:

1. **Main** — UI, window, orchestration. Trusted.
2. **Parse/render worker** — consumes the actual PDF bytes. Sandboxed, no
   filesystem or network capability, communicates over a narrow typed channel.

A malicious PDF that achieves code execution inside the worker lands somewhere
with nothing worth stealing. This costs an IPC hop per page render, which the
tile cache absorbs. The tradeoff is worth it: hostile-input parsing is the
project's single largest risk.

## Data flow: opening a document

1. Main process gets a path from the OS file dialog, opens a read-only handle.
2. Handle passes to the worker. Worker parses the xref/trailer only — not the
   whole file — and returns page count, dimensions, encryption status.
3. UI paints the page frame immediately from the dimensions.
4. Visible pages get requested at current zoom; worker rasterizes; tiles stream
   back and paint.
5. Nothing else is parsed until it's needed. A 900-page document opens as fast
   as a 2-page one.

Lazy parsing is what makes the startup budget achievable — it's an architectural
property, not an optimization to add later.

## Editing model

All mutations are **commands** against `easypdf-core`: a command has `apply`
and `invert`, which gives undo/redo for free and makes the mutation set
testable in isolation.

Saving is **incremental by default** — appended updates rather than a full
rewrite. This preserves bytes we don't understand, keeps existing digital
signatures valid, and is dramatically faster on large files. Full rewrite is
opt-in ("Save flattened / optimized").

## Frontend

TypeScript, no framework in v1. The UI is a canvas, a toolbar, a sidebar, and
a handful of dialogs — a framework would cost startup time and bundle size for
little benefit at this size. Revisit if the UI grows past what plain modules
handle comfortably; that reassessment is an honest maybe, not a rejection.

Rendering target is `<canvas>` with tiles blitted from the worker as raw
bitmaps. No PDF.js — the Rust side already renders; doing it twice is exactly
the bloat we're avoiding.

# Frontend Architecture

How the UI is put together, and how to add to it without editing it.

## Why this exists

The frontend grew to about 1,300 lines in one module with a single shared
mutable object. It worked, and it would not have survived Phase 2.

Phase 2 adds annotations, page operations, form filling and undo. Each of those
wants to draw on a page, handle pointer events, or occupy the sidebar — and in
the old shape, each would have meant editing the same three functions, with
every addition needing to know about all the others. Search highlights and text
selection were already hard-coded into one drawing function; annotation would
have been the third case in it, ink the fourth.

So the shape changed before the features arrive rather than after.

**No framework.** TD-006 still holds — the store below is about sixty lines.
The problem was never a lack of framework, it was a lack of seams.

## The pieces

```
src/
├── main.ts              composition root: wiring only, no logic
├── state.ts             the store: state + subscribe/notify
├── ipc.ts               typed wrappers over every Tauri command
├── viewer/
│   ├── viewer.ts        slots, render queue, overlay canvas
│   ├── layers.ts        registry: things drawn on a page
│   └── tools.ts         registry: pointer tools
├── ui/
│   ├── toolbar.ts       reacts to the store
│   ├── panels.ts        registry: sidebar panels
│   └── dialogs.ts       modals
└── features/
    ├── document.ts      open/close, password flow
    ├── navigation.ts    page, zoom, rotation, view mode
    ├── search.ts        a layer
    ├── selection.ts     a layer + a tool
    ├── outline.ts       a panel
    └── thumbnails.ts    a panel
```

## The three seams

**Layers** — anything drawn over a page. A layer declares a name, a draw order,
whether it has content on a given page, and how to draw it. The viewer sizes
the overlay canvas, handles the PDF-to-canvas coordinate flip, and skips the
canvas entirely when no layer has anything to draw.

The flip lives in `layers.ts` and nowhere else, deliberately: every layer needs
it, and every layer getting it wrong independently is how highlights end up on
the opposite side of the page.

**Tools** — pointer behaviour. One tool is active; the viewer routes
`pointerdown/move/up` to it with the position already converted to PDF page
coordinates. Selection is a tool. Highlight, ink, note and shape tools will be
tools, and none of them will need to negotiate with selection for the events.

**Panels** — the sidebar. A panel owns its DOM and is mounted on first show.
The registry handles switching, and the toolbar builds its own buttons from
what is registered.

## Adding a feature

Thumbnails were built after the refactor as a test of it, and needed no changes
to the viewer, the toolbar, or any layout code. That was the acceptance
criterion: if a new panel had required edits elsewhere, the seam would not have
been worth having.

A highlight annotation tool, concretely:

1. `registerLayer` to draw existing highlights
2. `registerTool` to create one by dragging
3. add its state to the store, and `notify` when it changes

No edits to `viewer.ts`, `toolbar.ts` or `main.ts` beyond one registration call.

## Rules learned the hard way

**Never settle a promise on a DOM event you did not cause.** `dialog.close()`
does not always dispatch `close`; the password dialog hung forever on that.
Settle where the user's intent is known.

**Do not trust observer APIs to fire.** `requestAnimationFrame` does not fire in
a minimised window. An unreferenced `ResizeObserver` is collected and silently
stops. `IntersectionObserver` does not fire at all in the test browser. The
viewer and the thumbnail panel both measure visibility explicitly instead, and
where an observer is used it is held in a variable and treated as an
enhancement.

**Force layout before measuring it.** Reading offsets straight after building
elements gives zero for everything, which reads as "all of it is on screen" —
that rendered 200 pages twice, once in the viewer and once in thumbnails.
`void element.offsetHeight` flushes layout. A hidden container measures as zero
too, so panels are shown before they are refreshed.

**A promise chain used as a queue must not be poisoned.** `chain.then(task)`
skips every later `.then` after one rejection. `chain.catch(() => {}).then(task)`.

**Zero a canvas you are not using.** An untouched `<canvas>` still allocates
300x150. Across hundreds of slots that is tens of megabytes for pages nobody
has looked at.

## What the harness can and cannot see

`apps/desktop/harness/` runs the real bundle in a browser with Tauri stubbed,
which is the only way to catch a whole class of failure — module resolution,
DOM wiring, canvas behaviour — that no Rust test can reach.

It stubs the IPC boundary, so it cannot catch a mismatch in behaviour behind a
matching shape. `scripts/check-contracts.mjs` checks the shapes against the real
Rust types; the session integration tests cover the behaviour.

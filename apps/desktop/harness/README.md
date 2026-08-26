# Frontend harness

Loads the **real** frontend bundle in a plain browser with Tauri stubbed out,
so the UI can be exercised without building or running the app.

```bash
npm --prefix apps/desktop run build
python3 -m http.server 8731 --directory apps/desktop
# then open http://localhost:8731/harness/
```

## Why it exists

The frontend once shipped completely inert: `tsc` emitted bare module
specifiers that no webview can resolve, so the script never ran — no buttons,
no rendering, nothing. No Rust test could have caught it, because the failure
was entirely in the browser.

Since then the harness has caught, among others:

- a search returning no matches leaving the previous highlights on screen
- `requestAnimationFrame` never firing, so a document opened in a background
  window would have stayed blank
- a render queue that died permanently after one rejection
- 36 MB of canvas backing store held for pages nobody had opened
- a password dialog that hung forever because `dialog.close()` does not always
  dispatch `close`

## The stubs are checked

`stubs.js` is a module, not inline script, so `scripts/check-contracts.mjs` can
import it and compare every stub against fixtures generated from the real Rust
types. This matters: the render stub once ignored rotation, which would have
let the rotation path pass its own test while doing nothing.

```bash
cargo test -p easypdf-session --test contract   # regenerate fixtures
node scripts/check-contracts.mjs                # check the stubs against them
```

## What it cannot catch

It stubs the IPC boundary, so it cannot detect a mismatch in *behaviour* behind
a matching shape. The Rust side is covered separately by the session
integration tests, which drive a real worker and real PDFium.

// Tauri stand-in for the browser harness.
//
// Exported as a module rather than inlined in the page so that
// `scripts/check-contracts.mjs` can import it and compare every stub's shape
// against the fixtures Rust generates. A stub that drifts from what the app
// actually sends makes the harness pass while the app is broken — which has
// already happened once, when the render stub ignored rotation and the
// rotation path would have passed its own test while doing nothing.

/** Mutable knobs the harness page and the tests both poke at. */
export function createState() {
  return {
    calls: [],
    errors: [],
    /** Set to a command name to make it reject, simulating a denied permission. */
    failCommand: null,
    pageCount: 3,
    /** Set to a string to simulate an encrypted document. */
    requirePassword: null,
    outline: [],
    engineAvailable: true,
  };
}

/** A synthetic page: an 8-byte header then RGBA pixels, as Rust sends. */
export function fakePage(width, height) {
  const buffer = new ArrayBuffer(8 + width * height * 4);
  const view = new DataView(buffer);
  view.setUint32(0, width, true);
  view.setUint32(4, height, true);

  const pixels = new Uint8Array(buffer, 8);
  for (let i = 0; i < pixels.length; i += 4) {
    const dark = i % 400 < 40;
    pixels[i] = dark ? 20 : 255;
    pixels[i + 1] = dark ? 20 : 255;
    pixels[i + 2] = dark ? 20 : 255;
    pixels[i + 3] = 255;
  }
  return buffer;
}

/**
 * Every stubbed command, keyed by name.
 *
 * Each returns exactly the shape the corresponding Rust command returns. The
 * contract checker verifies that claim rather than trusting it.
 */
export function handlers(state) {
  return {
    worker_status: () => ({
      running: true,
      sandboxed: true,
      detail: "confined via seatbelt",
      memoryCapped: false,
      engineAvailable: state.engineAvailable !== false,
    }),

    "plugin:dialog|open": () => "/fake/example.pdf",

    open_document: (args) => {
      if (state.requirePassword && args.password !== state.requirePassword) {
        // Matches OpenError's shape, which the frontend branches on.
        throw { needsPassword: true, message: "This document is password protected." };
      }
      return {
        name: "example.pdf",
        pageCount: state.pageCount ?? 3,
        encrypted: Boolean(state.requirePassword),
      };
    },

    // Always unrotated, as the real command returns.
    page_size: () => ({ width: 200, height: 100 }),

    render_page: (args) => {
      // Mirrors the real renderer: a quarter turn swaps the axes.
      const quarter = args.rotation === 90 || args.rotation === 270;
      const width = quarter ? 100 : 200;
      const height = quarter ? 200 : 100;
      return fakePage(Math.round(width * args.zoom), Math.round(height * args.zoom));
    },

    search: (args) => {
      if (!args.query || args.query === "nomatch") return { hits: [], truncated: false };
      // One hit near the page bottom, one near the top, so a test that gets
      // the PDF-to-canvas coordinate flip wrong is caught by position.
      return {
        hits: [
          { page: 0, rects: [{ left: 20, bottom: 10, right: 90, top: 30 }] },
          { page: 2, rects: [{ left: 20, bottom: 80, right: 90, top: 95 }] },
        ],
        truncated: false,
      };
    },

    text_layout: () => {
      const word = "Hello EasyPDF";
      return {
        chars: [...word].map((character, index) => ({
          text: character,
          rect: { left: 20 + index * 12, bottom: 40, right: 20 + index * 12 + 12, top: 64 },
        })),
        truncated: false,
      };
    },

    extract_text: () => "Hello EasyPDF",
    outline: () => state.outline,
    close_document: () => null,
  };
}

/** Builds the `window.__TAURI_INTERNALS__` stand-in. */
export function createInternals(state) {
  const commands = handlers(state);
  return {
    transformCallback: (callback) => callback,
    unregisterCallback: () => {},
    invoke: async (cmd, args = {}) => {
      state.calls.push({ cmd, args });
      if (state.failCommand === cmd) {
        throw new Error(`${cmd} not allowed. Permissions associated with this command: ...`);
      }
      const handler = commands[cmd];
      if (handler === undefined) throw new Error(`unstubbed command: ${cmd}`);
      return handler(args);
    },
  };
}

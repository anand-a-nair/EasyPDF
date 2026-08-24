# Technical Decisions

ADR-style. Each records what was chosen, why, what was rejected, and what would
make us revisit. Decisions are dated so it's clear what information was
available at the time.

---

## TD-001 — Rust for the core

**Date:** 2026-08-24 · **Status:** accepted

Rust for all document parsing, editing, and cryptography.

**Why:** PDF parsing is hostile-input parsing. Essentially every serious CVE in
PDF readers over the last two decades has been a memory-safety bug — use-after-
free, heap overflow, type confusion in the object graph. Writing a new parser in
C or C++ in 2026 means volunteering for that history. Rust removes the entire
bug class from code we write, which matters more here than in most applications.

Secondary: excellent cross-compilation to macOS/Windows/Linux on x86-64 and
ARM64, no runtime to ship, and a genuinely good crate ecosystem for this domain.

**Rejected:** C++ (mature libraries, but inherits the CVE class), Go (GC pauses
during render, larger binaries, weaker FFI), C# / .NET (runtime dependency
fights the size budget).

**Revisit if:** never, realistically. This is the load-bearing decision.

---

## TD-002 — Tauri, not Electron

**Date:** 2026-08-24 · **Status:** accepted

Desktop shell is Tauri, using each OS's built-in WebView.

**Why:** it is the difference between the project's stated goal and its
opposite. Electron bundles Chromium: ~120–200 MB installed, ~100 ms+ of runtime
startup before your code runs, and 150–300 MB idle RSS. Tauri uses WebView2 on
Windows and WKWebView on macOS, both already present, putting binaries in the
5–15 MB range.

The honest cost: **WebView differences are now your compatibility matrix.**
WKWebView and WebView2 disagree on details, and Linux's WebKitGTK is the weakest
of the three. Budget real time for per-platform UI bugs. This is a genuine tax,
accepted knowingly, because the alternative violates the first principle in
[01-vision.md](01-vision.md).

**Rejected:** Electron (fails the size budget by an order of magnitude), Qt
(smallest binaries and a fine choice, but slower iteration and a C++ surface),
egui/iced (leanest possible, but hand-building every widget delays a usable UI
by roughly a year).

**Revisit if:** WebView fragmentation costs more engineering time than the
binary-size win is worth. Track it honestly rather than defending the choice.

---

## TD-003 — PDFium for rendering

**Date:** 2026-08-24 · **Status:** accepted

Rasterization via PDFium, bound through `pdfium-render`.

**Why:** PDFium is the engine in Chrome. It is the most battle-tested PDF
renderer in existence, handles the real-world corpus of malformed PDFs that
theory says shouldn't exist, and is **BSD-3-licensed** — which is what keeps
our own license options open.

Writing a renderer from scratch means implementing the full graphics model,
CMaps, font subset handling, blend modes, transparency groups, and shading
types. That is a multi-year project on its own and would be the whole product.

**The tradeoff, stated plainly:** PDFium is C++, so it reintroduces the memory-
safety risk that TD-001 removed. This is exactly why the sandboxed worker
process in [02-architecture.md](02-architecture.md) exists — it is not optional
architectural garnish, it is the mitigation for this specific decision.

**Rejected:** MuPDF (excellent and compact, but **AGPL** — would force copyleft
on the whole project), Poppler (**GPL**, same problem), pure-Rust renderers
(promising, not yet close to the compatibility bar).

**Revisit if:** a pure-Rust renderer reaches real-world compatibility. That
would let us drop the sandbox complexity entirely. Worth checking yearly.

---

## TD-004 — Incremental save by default

**Date:** 2026-08-24 · **Status:** accepted

Writes append an incremental update section rather than rewriting the file.

**Why:** three benefits at once. It preserves bytes whose semantics we don't
model (so we can't corrupt them), it keeps existing digital signatures valid
(a full rewrite invalidates every prior signature), and it's O(change) rather
than O(file) — fast on a 400 MB scanned document.

**Cost:** files grow with each save. Mitigated by offering explicit
"Save optimized" that rewrites and compacts.

---

## TD-005 — No plugin API in v1

**Date:** 2026-08-24 · **Status:** accepted

**Why:** a plugin API freezes internal interfaces before we know if they're
right, and adds a code-execution surface to a security-sensitive app. Both costs
are permanent; the benefit is speculative until there's a user base asking.

**Revisit if:** there's concrete demand and the core APIs have been stable for
a couple of releases.

---

## TD-006 — TypeScript frontend, no framework

**Date:** 2026-08-24 · **Status:** accepted, low confidence

**Why:** the v1 UI is small enough that a framework's cost (bundle size, startup
parse, build complexity) outweighs its benefit.

**Revisit if:** the UI outgrows plain modules. Flagged as low confidence
deliberately — this is the decision here most likely to be wrong, and reversing
it is cheap compared to the others.

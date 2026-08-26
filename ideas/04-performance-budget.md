# Performance Budget

"Lightweight" is meaningless without numbers. These are the numbers. They have
veto power over features: a feature that breaks a budget doesn't ship, or
something else gets removed to pay for it.

## The budget

| Metric | Budget | Hard fail |
|---|---|---|
| Installer size (per platform) | ≤ 15 MB | 25 MB |
| Installed size | ≤ 40 MB | 60 MB |
| Cold start → window visible | ≤ 400 ms | 700 ms |
| Cold start → first page painted (10 MB doc) | ≤ 800 ms | 1.5 s |
| Idle RSS, one document open | ≤ 150 MB | 250 MB |
| Page navigation (cached) | ≤ 16 ms | 33 ms |
| Zoom step re-render, visible page | ≤ 100 ms | 250 ms |
| Open 500-page document → interactive | ≤ 1 s | 2 s |
| Save (incremental, small edit, 50 MB file) | ≤ 200 ms | 500 ms |

Measured on the stated baseline machine, not a developer workstation.

## Measured, 2026-08-25

First full measurement against the budget. Release build, macOS on Apple
Silicon — **not** the baseline machine below, so these are optimistic; the
point is that the budget is now measured rather than aspirational.

| Metric | Budget | Measured | |
|---|---|---|---|
| Installer size | ≤ 15 MB | **4.77 MB** | ✅ |
| Cold start → window | ≤ 400 ms | **167–207 ms** | ✅ |
| Idle RSS, no document | ≤ 150 MB | **95–103 MB** | ✅ |
| 500-page open → first page | ≤ 1 s | **16.6 ms** | ✅ |
| Cached page navigation | ≤ 16 ms | **2.5 µs** | ✅ |
| Zoom step re-render | ≤ 100 ms | **4.3 ms** | ✅ |
| Cold page render, average | ≤ 16 ms | **968 µs** | ✅ |
| Search across 500 pages | — | **6.5 ms** | ✅ |

Two results worth keeping:

**Opening 500 pages (2 ms) is faster than opening one page (9 ms).** The
single-page case pays PDFium's one-time library warm-up. This is direct
evidence for the architectural claim in
[02-architecture.md](02-architecture.md) that opening reads the cross-reference
table rather than walking the page tree — and
`opening_does_not_scale_with_page_count` now guards it.

**Idle memory is ~100 MB before a document is even open**, two thirds of the
budget. Almost all of it is the system WebView. It is the figure most likely to
push past budget later, and the one least under our control — worth watching
rather than assuming the headroom is real.

**Search needs no index.** 6.5 ms across 500 pages, against a 250 ms input
debounce. A text index was on the roadmap as a performance fix; the measurement
says it would be solving a problem that does not exist. Revisit if a large
scanned document says otherwise.

## What is still unmeasured

- The **400 MB scanned book** case. The corpus has no such document; generating
  a realistic one is not the same as having one.
- Anything on the **baseline machine** below. Every figure here is from a fast
  laptop.
- Linux and Windows, which have never been built.

## Baseline machine

Budgets are validated against modest hardware, because that's where the
difference is felt: **4-core CPU, 8 GB RAM, SATA SSD** — roughly a 2018 laptop.
If it's pleasant there, it's invisible on current hardware. Benchmarking only on
an M-series Mac is how projects convince themselves they're fast.

## Enforcement

Budgets that aren't measured automatically are aspirations. CI must:

1. **Fail the build on binary-size regression** beyond a small threshold. Size
   creeps one dependency at a time; this is the only reliable defense.
2. **Track startup time** across a fixed corpus, with results posted to the PR.
3. **Run a benchmark suite** on a pinned document set (see below) and flag
   regressions over ~10%.
4. **Assert zero unexpected network activity** in an integration test. The
   local-only promise in [01-vision.md](01-vision.md) is testable, so test it.

## Test corpus

A fixed set of documents, committed or pinned by hash, covering the cases that
actually stress things:

- A 2-page text-only document (the common case; startup path)
- A 500-page text document (page-tree scaling)
- A 400 MB scanned book (memory ceiling, tile cache pressure)
- A form-heavy government PDF (AcroForm handling)
- A CJK document with embedded subset fonts (font handling)
- A document with transparency groups and blend modes (render correctness)
- A signed document (signature preservation across saves)
- An encrypted document, each supported cipher
- Several deliberately malformed files (see [07-security.md](07-security.md))

## Where the budget usually goes

Recording the known threats so they're recognized on arrival:

- **Dependency creep.** Each convenience crate brings transitive weight. Audit
  `cargo tree` before adding anything.
- **The frontend bundle.** The reason for TD-006. A framework plus a component
  library would consume most of the size budget for a UI this small.
- **Eager parsing.** Any change that parses the whole document at open, rather
  than lazily, breaks the large-document budgets immediately.
- **Tile cache without a ceiling.** Needs a hard memory cap with LRU eviction
  from day one, not after the first out-of-memory report.

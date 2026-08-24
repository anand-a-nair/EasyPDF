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

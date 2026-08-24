# EasyPDF

**A fast, local, open source PDF tool for macOS, Windows, and Linux.**

View, edit, annotate, fill, sign, and encrypt PDFs — without a subscription,
without an account, and without your documents leaving your machine.

> **Status: pre-alpha.** Design and planning are underway; there's nothing to
> install yet. The thinking lives in [`ideas/`](ideas/) and is worth reading if
> you're considering contributing.

## Why

Existing PDF tools tend to be one of three things: subscription software that
launches slowly and pushes cloud features you didn't ask for, web uploaders that
want your document on someone else's server, or free desktop tools that do one
thing and stopped being maintained in 2019.

There's no obvious answer to *"I need to sign this and send it back"* that is
fast, local, free, and trustworthy. That's the gap.

## Principles

- **Local by default** — no account, no telemetry, no network calls you didn't
  ask for. Enforced by a test, not just a promise.
- **Genuinely lightweight** — a ~15 MB installer and sub-400 ms startup are
  budgeted constraints with veto power over features, not aspirations. See the
  [performance budget](ideas/04-performance-budget.md).
- **Correct before complete** — a document we can't handle gets an honest error,
  never silent corruption.
- **Restraint** — every feature costs startup time, size, and attack surface.

## Planned capabilities

| | |
|---|---|
| **View** | Fast rendering, text selection, search, thumbnails, outlines |
| **Pages** | Reorder, rotate, delete, insert, extract, merge, split |
| **Annotate** | Highlight, ink, notes, shapes, text boxes, image stamps |
| **Forms** | Fill and save AcroForm fields |
| **Sign** | Visible signatures + real cryptographic signing (PAdES) |
| **Secure** | AES-256 encryption, password management, permission flags |
| **Optimize** | Compression, image downsampling, OCR for scanned documents |

Text editing — reflowing existing body text — is deliberately last and
deliberately modest in scope. [Here's why that's hard](ideas/06-features.md).

## Stack

**Rust** core with a **Tauri** shell, rendering via **PDFium**.

Rust because PDF parsing is hostile-input parsing and memory safety is the whole
ballgame. Tauri because it uses the OS's built-in WebView — the difference
between a 15 MB app and a 200 MB one. PDFium because it's the most battle-tested
renderer available and BSD-licensed. The full reasoning, including what was
rejected, is in [tech decisions](ideas/03-tech-decisions.md).

## Repository layout

```
EasyPDF/
├── ideas/              design docs — vision, architecture, decisions, roadmap
├── .version            canonical version (all manifests derive from it)
├── rust-toolchain.toml pinned toolchain
├── CHANGELOG.md        public release history
├── SECURITY.md         vulnerability disclosure
└── LICENSE / NOTICE    Apache-2.0
```

Code directories arrive with Phase 0; see the [roadmap](ideas/05-roadmap.md).

## Contributing

Early, and the design is still soft — [CONTRIBUTING.md](CONTRIBUTING.md) is the
place to start, and the [open questions](ideas/08-open-questions.md) are where
input is most useful right now.

## License

[Apache-2.0](LICENSE). Permissive, with an explicit patent grant. Details and
dependency compatibility rules in [ideas/09-licensing.md](ideas/09-licensing.md).

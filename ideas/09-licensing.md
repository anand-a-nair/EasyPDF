# Licensing

**EasyPDF is licensed under Apache-2.0.** Full text in [../LICENSE](../LICENSE).

## Why Apache-2.0

Permissive, so anyone can use, fork, or ship it — including commercially, which
maximizes the odds it gets adopted and maintained. Unlike MIT, it includes an
**explicit patent grant** from contributors and a retaliation clause. For a
file-format tool that matters more than usual: PDF itself is royalty-free under
ISO 32000, but adjacent technologies embedded in PDFs (JBIG2, JPEG2000, some
cryptographic constructions) have patent history, and explicit beats implied.

It also keeps distribution options open — notably app stores, whose terms
conflict with GPL's no-additional-restrictions clause. VLC had to relicense for
exactly this reason.

## Dependency compatibility

Every dependency must be compatible with Apache-2.0 distribution. Checked in CI
via `cargo-deny` with an allowlist.

| Component | License | Status |
|---|---|---|
| PDFium | BSD-3-Clause | ✅ Compatible — the reason this stack works |
| Tauri | MIT / Apache-2.0 | ✅ |
| Rust stdlib & most crates | MIT / Apache-2.0 | ✅ |
| **MuPDF** | **AGPL-3.0** | ❌ **Do not use** — would force copyleft project-wide |
| **Poppler** | **GPL-2.0/3.0** | ❌ **Do not use** — same |
| **qpdf** | Apache-2.0 | ✅ Compatible if we ever want it |
| Tesseract (OCR) | Apache-2.0 | ✅ — see OQ-003 for the size question |

**The standing rule:** GPL and AGPL components are excluded, not out of
hostility to copyleft but because adopting one silently relicenses the entire
project. If a copyleft library is genuinely the only option for something, that
is a project-level decision, made explicitly in
[03-tech-decisions.md](03-tech-decisions.md) — never a quiet dependency addition.

## Applying the license

Source files carry the standard short header:

```rust
// Copyright 2026 EasyPDF Contributors
// Licensed under the Apache License, Version 2.0.
// See the LICENSE file in the project root for details.
```

Copyright is held by contributors individually — no CLA or copyright assignment
currently required. See OQ-006.

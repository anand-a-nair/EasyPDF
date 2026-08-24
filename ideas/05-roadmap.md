# Roadmap

Phased so that each phase is independently useful. If the project stopped at the
end of any phase, what exists would still be worth having.

## Phase 0 — Foundation

Repo, license, CI, crate skeleton. Sandboxed worker process and the IPC protocol
established early — retrofitting a security boundary is painful, and the
architecture in [02-architecture.md](02-architecture.md) assumes it.

Exit: `cargo build` produces a window; CI runs tests and reports binary size.

## Phase 1 — Excellent viewer

The whole phase is "open and read a PDF, better than anything else at this size."

- Open, render, scroll, zoom, rotate; progressive render with tile cache
- Text selection, copy, in-document search
- Thumbnail sidebar, outline/bookmarks, page jump
- Encrypted-document password prompt (read path)
- Platform packaging: `.dmg`, `.msi`, AppImage/`.deb`

Exit: all Phase 1 performance budgets met on the baseline machine.

## Phase 2 — Page operations and annotation

The bulk of what people mean by "edit a PDF."

- Reorder, rotate, delete, insert, extract pages; merge and split documents
- Annotations: highlight, underline, strikeout, freehand, notes, shapes
- Text boxes and image stamps (overlay — *not* editing existing text)
- Fill AcroForm fields
- Undo/redo across all of it; incremental save

Exit: a person can receive a form, fill it, annotate it, and send it back.

## Phase 3 — Signing and encryption

- Visible signature placement (drawn, typed, or image)
- Cryptographic signing: PKCS#7 detached, PAdES B-B and B-T with timestamping
- Certificate import; signature verification with a clear, honest trust UI
- Set/remove passwords; AES-256 encryption; permission flags

Exit: a signed document verifies correctly in Acrobat and in EasyPDF.

## Phase 4 — Optimization and OCR

- Compression and linearization ("Save optimized")
- Image downsampling with quality control
- OCR for scanned documents, producing a searchable text layer
- Export: PDF→image, PDF→text

OCR needs a decision on engine and on whether its model files fit the size
budget — likely an optional download rather than a bundled asset.

## Phase 5 — Text editing (experimental)

The genuinely hard one, deliberately last, with expectations set low.

PDF has no concept of a paragraph. Text is positioned glyph runs drawn with
subset-embedded fonts that frequently lack the glyphs you'd need to type a new
character. Editing a sentence can require re-subsetting a font, re-flowing a run,
and hoping the surrounding layout tolerates it.

Realistic scope: edit text within a single run, same font, same size, with
graceful refusal when the embedded subset lacks the needed glyphs. Anything
promising more than that is overselling, and every open source tool that has
promised more has disappointed.

Exit: correct edits on simple documents, honest refusal on the rest. "Refuses
clearly" is a successful outcome here, not a failure.

## Explicitly not scheduled

Cloud sync, accounts, collaborative editing, plugin marketplace, mobile apps.
See the non-goals in [01-vision.md](01-vision.md).

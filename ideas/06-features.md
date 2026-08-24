# Feature Specifications

Detail on the areas where "how hard is this, really?" has a non-obvious answer.

## Viewing

The foundation, and the thing most users judge the app by.

**Progressive render.** Request a page, get a low-resolution tile within
~50 ms, get the sharp version when it's ready. Users read perceived latency,
not actual latency; a blurry page immediately beats a crisp page in 300 ms.

**Tile cache.** Keyed by (page, zoom bucket, rotation). Hard memory ceiling with
LRU eviction — set at launch, not after the first bug report. Zoom buckets are
quantized so small zoom changes reuse tiles.

**Text selection** needs the glyph-position map from PDFium, plus reading-order
heuristics. Multi-column layouts are where naive implementations embarrass
themselves; worth testing against academic papers specifically.

**Search** over extracted text with a per-document index built lazily on first
search. Ligature and hyphenation normalization needed for decent recall.

## Page operations

Mechanically the easiest wins in the project — page-tree manipulation on the
object graph, no rendering involved. Reorder, rotate, delete, insert, extract,
merge, split.

The one subtlety: pages carry references to shared resources (fonts, color
spaces, XObjects). Deleting a page must not orphan resources another page uses,
and extracting a page must bring its dependencies with it. Get the reference
counting right once, centrally, and every operation inherits it.

## Annotation

Annotations are standard PDF objects, so this is object-graph work rather than
rendering work. Highlight, underline, strikeout, squiggly, freehand ink, sticky
notes, shapes, text boxes, image stamps.

Two details that separate good from adequate: annotations must round-trip
through other readers correctly (write proper appearance streams — some readers
won't render annotations that lack them), and freehand ink needs curve smoothing
or it looks amateurish on a trackpad.

## Form filling

AcroForm fields: text, checkbox, radio, choice, signature. Read the field tree,
render widget appearances, write values back, regenerate appearance streams.

XFA forms — Adobe's XML-based alternative — are a separate and much larger
implementation. Deprecated in PDF 2.0 and rare outside certain government
systems. **Decision: detect and refuse with a clear message.** Silently
mishandling them is worse than declining.

## Signing

Two distinct things that users conflate, and the docs and UI should not:

**Visible signature** — an image or drawing of a signature placed on the page.
Cosmetic. No cryptography. What most people mean by "sign this."

**Digital signature** — a cryptographic PKCS#7 detached signature over the
document's byte ranges, proving integrity and identity. What "signed" means
legally in most jurisdictions.

Implementation: reserve a byte range, compute the digest, produce the PKCS#7
blob, splice it in. Fiddly but well-specified. Target PAdES B-B (basic) and
B-T (with an RFC 3161 timestamp); B-LT/B-LTA later if there's demand.

**Verification UI is the hard part, and it's a design problem more than a
crypto one.** "Signature valid" means several separable things: the bytes are
unmodified, the certificate chains to a trusted root, the certificate wasn't
revoked at signing time, the timestamp is trustworthy. Collapsing these into a
green checkmark is how users get misled. Show what was actually verified, and
say plainly what wasn't.

## Encryption

The standard security handler. Support RC4 40/128 and AES-128 for reading legacy
documents; **write AES-256 only** — offering weak ciphers for new documents is
a footgun with no upside.

Two password types with different meanings: *user password* (required to open)
and *owner password* (permits changing permissions). Permission flags — no
print, no copy, no modify — are worth implementing correctly while being honest
in the docs that they are advisory. Any tool with the decryption key can ignore
them, and ours technically could too. We honor them; we don't pretend they're
security.

## Text editing (Phase 5)

Covered in [05-roadmap.md](05-roadmap.md). Restating the constraint because it's
the feature most likely to be over-promised: PDF stores positioned glyphs, not
paragraphs. Fonts are usually subset — the file physically contains only the
glyphs the document already uses, so typing a character the document never used
may have nothing to draw with.

Realistic v1: edit within a single text run, same font, same size, refusing
clearly when the embedded subset can't supply a glyph. That covers fixing a typo
or a date, which is the majority of real demand, and nothing more.

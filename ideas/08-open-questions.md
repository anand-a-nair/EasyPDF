# Open Questions

Tracked so they're decided deliberately rather than by accident. Each notes what
would resolve it and roughly when it needs an answer.

## OQ-001 — How is PDFium obtained and shipped?

Building PDFium from source is a heavy lift (depot_tools, a long build). Using
prebuilt binaries from a third-party release means trusting that supply chain
for our most security-sensitive dependency, which sits uncomfortably next to
T5 in [07-security.md](07-security.md).

Options: build from source in CI and cache; vendor a pinned prebuilt with hash
verification and a documented provenance; or dynamic-link to a system copy where
one exists.

**Needed by:** Phase 0. It shapes CI and the build.

## OQ-002 — Linux WebView story

WebKitGTK is the weakest of the three WebViews Tauri targets, and it varies
across distributions. May cost disproportionate effort relative to Linux's share
of users.

Options: support it properly; ship Linux as best-effort with known issues
documented; or delay Linux to post-1.0.

**Needed by:** Phase 1 packaging. Answer by measuring actual breakage, not by
guessing.

## OQ-003 — OCR engine and the size budget

Tesseract is the obvious choice but its language models are large — bundling
even English would consume much of the installer budget in
[04-performance-budget.md](04-performance-budget.md).

Leading option: optional download on first use, with the app fully functional
without it. Needs a decision on where models are fetched from, which brushes
against the no-network principle (user-initiated, so probably acceptable — but
it deserves an explicit carve-out rather than a quiet exception).

**Needed by:** Phase 4.

## OQ-004 — Code signing and notarization

Shipping on macOS and Windows without signing means scary OS warnings. Apple
Developer membership and a Windows code-signing certificate both cost real money
annually.

Who holds the certificates for an open source project, and who pays? This is a
governance question as much as a technical one, and unsigned binaries would
undercut a tool whose pitch is trustworthiness.

**Needed by:** first public release. Start early — notarization setup is
tedious and certificate issuance can take weeks.

## OQ-005 — Update mechanism

Auto-update conflicts with the no-network principle. But a security-sensitive
app where users never learn about patches is worse.

Leading option: a check that is opt-in on first run, clearly explained, off by
default, contacting only the release host and sending nothing but a version
query. Package managers handle it on Linux.

**Needed by:** first public release.

## OQ-006 — Project governance

Who merges, how decisions get made, whether there's a CLA. Apache-2.0's
contributor patent grant covers much of what a CLA is usually for, so a CLA may
be unnecessary friction. Worth deciding before the second contributor, not after.

**Needed by:** when external contributions start.

## OQ-007 — Name and trademark

"EasyPDF" is generic and near-certainly used elsewhere. Fine for a hobby
project; a problem if it grows. Worth a search before investing in branding.

**Needed by:** before a public launch with any marketing behind it.

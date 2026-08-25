# Open Questions

Tracked so they're decided deliberately rather than by accident. Each notes what
would resolve it and roughly when it needs an answer.

## OQ-001 — How is PDFium obtained and shipped?

**Status: provisionally answered 2026-08-24 — vendoring a pinned prebuilt.**
See TD-007. This is a momentum decision, not a final one: it trades a
supply-chain risk on our most security-sensitive dependency for the ability to
make progress now. **It must be resolved properly before any public release**,
either by reproducing the build ourselves or by auditing provenance thoroughly.
Leaving it provisional at 1.0 is a release blocker.

The original framing, retained because the tradeoff hasn't changed:


Building PDFium from source is a heavy lift (depot_tools, a long build). Using
prebuilt binaries from a third-party release means trusting that supply chain
for our most security-sensitive dependency, which sits uncomfortably next to
T5 in [07-security.md](07-security.md).

Options: build from source in CI and cache; vendor a pinned prebuilt with hash
verification and a documented provenance; or dynamic-link to a system copy where
one exists.

**Needed by:** resolved-for-now for Phase 0; full answer needed by first release.

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

## OQ-008 — How does the worker binary ship alongside the app?

**Answered 2026-08-25 for macOS.** The worker ships as a Tauri sidecar
(`externalBin`) and PDFium as a bundled resource, staged by
`scripts/prepare-bundle.sh`. Verified by driving the *bundled* worker over its
real protocol: it reports `engine_available: true`, confines itself via
seatbelt, opens a document and renders it. A bundle with the engine removed
reports `engine_available: false` and refuses every document, with no fallback.

Installer: **4.77 MB**, against a 15 MB budget.

Windows and Linux bundles are configured and wired into CI but have never been
built. The original framing follows.


The app locates `easypdf-worker` beside its own executable. That works in
development, where both land in `target/<profile>/`, but bundling is unsolved:
Tauri's `externalBin` sidecar mechanism expects a target-triple naming
convention, and the worker must end up inside the `.app`, `.msi`, and AppImage.

Getting this wrong fails in a specific and bad way: the app runs, finds no
worker, and would be tempted to fall back to in-process parsing — which would
silently discard the entire security model. **There must be no such fallback.**
No worker means no document, with a clear message.

**Needed by:** first bundled build, Phase 1.

## OQ-009 — Linux and Windows confinement

macOS confinement is implemented and verified. The other two report
`NotEnforced`, which means a worker there handles untrusted input with ordinary
user privileges.

Linux: landlock (filesystem) plus seccomp-bpf (syscalls). Windows: AppContainer
plus a job object, applied by the parent at spawn rather than by the child.

**Needed by:** before shipping on those platforms. Releasing a document tool
with no confinement on two of three targets would undercut the security
argument the project rests on.

## OQ-010 — Pass a file descriptor instead of document bytes

`OpenDocument` currently carries the document's bytes across the channel. That
works and is safe, but it copies the whole file and caps documents at the
256 MB frame ceiling — a real limit for the large scanned books in the test
corpus.

Descriptors inherited before confinement remain usable inside the sandbox, so
passing one would remove both the copy and the ceiling. It needs `SCM_RIGHTS`
on Unix and handle duplication on Windows, plus a host-side handle table.

**Needed by:** Phase 1, when large-document performance is measured against
the budget.

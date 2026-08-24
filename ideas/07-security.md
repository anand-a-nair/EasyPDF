# Security Model

EasyPDF parses untrusted binary files that arrive by email from strangers. That
is the whole threat model in one sentence, and it drives more architecture than
any feature does.

## Threats

**T1 — Malicious document achieving code execution.** The primary threat. PDF
parsing has a long CVE history: malformed xref tables, object type confusion,
integer overflows in image decoders, recursive reference loops. PDFium is
hardened but not immune, and it is C++ (see TD-003).

**T2 — Data exfiltration.** PDFs can specify external resource loads, submit
form data to URLs, and — in readers that support it — execute JavaScript.

**T3 — Resource exhaustion.** Decompression bombs, deeply nested object graphs,
pathological content streams that render forever.

**T4 — Signature spoofing.** Presenting a document as validly signed when it
isn't — via incremental-update attacks that modify content after signing, or
shadow attacks that pre-plant alternate content.

**T5 — Supply chain.** A compromised dependency in a security-sensitive local
app with filesystem access.

## Mitigations

**Sandboxed parse/render worker (T1).** All PDF bytes are handled in a separate
process with no filesystem access beyond the passed handle, no network, and OS
sandboxing (seatbelt on macOS, AppContainer/job objects on Windows, seccomp-bpf
+ namespaces on Linux). Code execution there yields a process with nothing worth
having. This is the mitigation for accepting a C++ renderer, and it's why the
boundary exists from Phase 0 rather than being added later.

**No network, ever (T2).** The app makes no outbound connections. External
resource references in documents are not fetched — they're ignored, and the UI
says so where it matters. **JavaScript in PDFs is not executed, ever, with no
setting to enable it.** It exists almost exclusively for attacks and for form
logic we can approximate safely. Enforced by an integration test asserting zero
sockets, per [04-performance-budget.md](04-performance-budget.md).

The one deliberate exception: RFC 3161 timestamping during signing, which is
explicitly user-initiated, goes to a user-configured URL, and sends only a hash.

**Hard limits (T3).** Caps on decompression ratio, object-graph depth, total
allocation, and per-page render time. Exceeding a limit shows a clear message
rather than hanging. Every limit is a constant in one place, tunable, not
scattered through the parser.

**Honest signature verification (T4).** Verify that signed byte ranges cover the
whole document — the incremental-update attack works by appending content
outside the signed range and hoping the reader shows the whole file as signed.
Report exactly what was verified, and show which document revision was signed
when a file has multiple. Never a bare green checkmark.

**Dependency discipline (T5).** Minimal dependency count (which the size budget
already pushes toward), `cargo-deny` and `cargo-audit` in CI, lockfiles
committed, and manual review of any new transitive dependency in the crypto or
parsing path.

## Fuzzing

Continuous fuzzing of the parser with `cargo-fuzz`, seeded from the malformed
corpus, running in CI on a schedule rather than per-commit. Every crash found
becomes a regression test. For a project whose main input is hostile files, this
is not optional polish — it is the primary way parser bugs get found before
users do.

## Reporting

`SECURITY.md` with a private disclosure channel before the first public release.
A security-sensitive tool without a way to report vulnerabilities privately is
asking for them to be reported publicly.

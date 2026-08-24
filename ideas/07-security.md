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
sandboxing. Code execution there yields a process with nothing worth having.
This is the mitigation for accepting a C++ renderer, and it's why the boundary
exists from Phase 0 rather than being added later.

**Status as of 2026-08-24:**

| Platform | Confinement | State |
|---|---|---|
| macOS | seatbelt (`sandbox_init`) | **Implemented and verified** |
| Linux | landlock + seccomp-bpf | Planned — worker reports `NotEnforced` |
| Windows | AppContainer + job object | Planned — worker reports `NotEnforced` |

The worker confines itself **before reading a single byte of input** — ordering
is the point, since confinement applied after parsing begins protects nothing.
It then reports what it actually managed to apply, and the host surfaces that in
the UI. Sandboxing that silently fails is worse than none, because everything
downstream assumes it holds.

Verification is by denial, not by return code: the worker's self-test attempts a
file read, a file write, and a socket bind, and the test asserts all three are
blocked. `sandbox_init` returning zero only proves the profile was accepted.

**Known gap — no kernel memory ceiling on macOS.** `RLIMIT_AS` and `RLIMIT_DATA`
both fail with `EINVAL` there, regardless of whether the hard limit is lowered
too. macOS simply does not implement an address-space ceiling. Memory
containment on that platform therefore rests entirely on the accounting limits
below, with no kernel backstop. This is reported honestly rather than assumed —
`ResourceLimits::memory_capped()` returns false and the UI says so.

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
scattered through the parser. Implemented in `easypdf-ffi`; a CPU-seconds
ceiling is also applied via `setrlimit` where supported.

**Frame limits.** The host reads length-prefixed frames from a process that may
be compromised. The declared length is validated *before* allocation, so a
hostile worker announcing a 4 GB frame costs nothing. Responses are also checked
for self-consistency — a pixel buffer whose length disagrees with its declared
dimensions is rejected rather than handed to a consumer that would read out of
bounds.

**Worker death is treated as adversarial.** A worker that times out, dies, or
violates the protocol is killed rather than retried in place, and restarting
produces a genuinely fresh process. The API makes this hard to get wrong:
`Worker::restart` consumes the handle, so a dead worker cannot be reused.

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

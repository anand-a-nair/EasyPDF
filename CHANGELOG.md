# Changelog

All notable changes to EasyPDF are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The canonical version is in [`.version`](.version).

> This is the **public, user-facing** changelog. Internal development history
> and session handoffs live in `.claude/CHANGELIST.md` (not committed).

## [Unreleased]

### Added
- Project foundation: Apache-2.0 license, contribution guidelines, security
  policy, code of conduct
- Design documentation in [`ideas/`](ideas/) — vision, architecture, technical
  decisions, performance budget, roadmap, feature specs, threat model, strategy,
  sustainability, build and release process
- Development scaffolding: pinned Rust toolchain, editor config, versioning

- Cargo workspace with four crates: `easypdf-core` (document model, page
  operations, undoable command stack), `easypdf-render` (memory-bounded LRU
  tile cache), `easypdf-crypto` (encryption policy, permissions, signature
  verification model), `easypdf-ffi` (sandboxed worker protocol and resource
  limits)
- Sandboxed worker process (`easypdf-worker`) that confines itself before
  reading any input. On macOS confinement is enforced via seatbelt and verified
  by denial — the worker cannot read files, write files, or open sockets.
  Linux and Windows confinement are not yet implemented and are reported as
  such rather than assumed.
- Length-prefixed worker protocol with allocation limits validated before use,
  timeout and death handling, and a restart path that cannot accidentally reuse
  a dead process
- PDF rendering via a hash-pinned, vendored PDFium build with no JavaScript
  engine compiled in, so document scripts cannot execute
- Tauri desktop shell with a dependency-free TypeScript frontend
- CI: three-OS build matrix, formatting, lints, tests, `cargo-deny` license and
  advisory gate, binary size budget, version-consistency check
- Nightly fuzzing workflow (placeholder until the parser exists)

### Notes
- Pages render inside the sandboxed worker, but are not yet displayed in the
  window — that is Phase 1 UI work. See the [roadmap](ideas/05-roadmap.md).

[Unreleased]: https://github.com/anandnair/easypdf/compare/main...HEAD

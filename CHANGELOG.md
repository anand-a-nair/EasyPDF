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
- Tauri desktop shell with a dependency-free TypeScript frontend
- CI: three-OS build matrix, formatting, lints, tests, `cargo-deny` license and
  advisory gate, binary size budget, version-consistency check
- Nightly fuzzing workflow (placeholder until the parser exists)

### Notes
- No PDF rendering yet — PDFium is not vendored. The window opens and reaches
  the Rust core; that is the extent of Phase 0. See the
  [roadmap](ideas/05-roadmap.md).

[Unreleased]: https://github.com/anandnair/easypdf/compare/main...HEAD

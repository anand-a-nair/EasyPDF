# Build and Release

## Toolchain

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.98.0 stable | pinned via `rust-toolchain.toml` |
| Cargo | 1.98.0 | ships with Rust |
| Node | 24.x | frontend + Tauri CLI |
| Xcode CLT | clang 21 | macOS linker; required |

Verified working on `aarch64-apple-darwin` as of 2026-08-24.

## Tauri CLI

Per-project, not global (TD-008):

```
npm install -D @tauri-apps/cli@latest
npx tauri info      # verifies the whole environment
```

`npx tauri info` prints Rust, WebView, and OS versions and flags anything
missing. It's the first thing to run when something is broken.

## The cross-compilation constraint

**Tauri apps cannot be cross-compiled.** Each build links against that
platform's native WebView and packaging tools:

| Target | Requires | WebView | Output |
|---|---|---|---|
| macOS | macOS host | WKWebView (system) | `.dmg`, `.app` |
| Windows | Windows host | WebView2 (system on Win11) | `.msi`, `.exe` |
| Linux | Linux host | WebKitGTK (must install) | AppImage, `.deb` |

Consequences, which shape Phase 0 rather than being deferred:

- CI must be a **three-runner matrix** from the start. GitHub Actions provides
  all three free for public repos.
- You cannot produce a Windows build from this laptop. Ever. Releases are cut
  by CI, not locally.
- Linux needs `libwebkit2gtk-4.1-dev`, `build-essential`, `libssl-dev`,
  `librsvg2-dev` and friends installed on the runner.

## Apple Silicon vs Intel

Only `aarch64-apple-darwin` is installed. Intel Mac support means adding
`x86_64-apple-darwin` and producing a universal binary via `lipo`. Phase 1
packaging concern; noted so it isn't discovered late.

## Versioning

Semantic versioning. The canonical version lives in `.version` at the repo root;
every other manifest (`Cargo.toml`, `package.json`, `tauri.conf.json`) is
derived from it so they cannot drift.

Pre-1.0, minor bumps may break things — that's what `0.x` means. After 1.0, the
**file format compatibility promise** matters more than the API: a document
written by any version must be readable by every later version.

## Release checklist (draft)

1. Performance budgets validated on baseline hardware — see
   [04-performance-budget.md](04-performance-budget.md)
2. Fuzzing run clean; malformed corpus passes
3. `cargo deny` and `cargo audit` clean
4. All three platform builds green
5. Signed and notarized (OQ-004)
6. `CHANGELOG.md` updated
7. `.version` bumped, tag pushed
8. **No provisional decisions outstanding** — TD-007 in particular

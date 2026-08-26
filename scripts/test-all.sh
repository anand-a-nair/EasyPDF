#!/usr/bin/env bash
# Runs every test layer, in the order that fails fastest.
#
# The layers, and what each one can catch that the others cannot:
#
#   format/lint     style and lint regressions
#   unit            logic inside a crate
#   render          real PDFium against real documents
#   worker boundary the sandboxed process: confinement, death, restart
#   session         what the app does with a document, end to end
#   contract        that the browser harness matches what Rust actually sends
#   frontend types  TypeScript correctness
#   dependencies    licences and advisories
#   bundle          that the shipped package is self-contained
#   budgets         startup time and idle memory, which need a running app
#
# The frontend's *behaviour* is exercised by the browser harness, which needs a
# browser and is not run from here — see apps/desktop/harness/README.md.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

skip_bundle=0
[[ "${1:-}" == "--no-bundle" ]] && skip_bundle=1

step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }

step "format and lint"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings

step "rust tests (unit, render, worker boundary, session)"
cargo test --workspace

# Budget assertions are release-only: a debug build's timings say nothing about
# the product, and asserting on them makes the suite fail for reasons unrelated
# to any change.
step "performance budgets (release)"
cargo test --release -p easypdf-session --test budget -- --test-threads=1

step "contract: harness stubs versus real payloads"
node scripts/check-contracts.mjs

step "frontend types"
npm --prefix apps/desktop run typecheck

step "dependencies"
if command -v cargo-deny >/dev/null 2>&1; then
    cargo deny check
else
    echo "cargo-deny not installed; skipping (CI runs it)"
fi

if (( skip_bundle )); then
    printf '\n\033[1mall checks passed\033[0m (bundle skipped)\n'
    exit 0
fi

step "bundle"
npm --prefix apps/desktop run tauri build -- --bundles app
bash scripts/verify-bundle.sh
bash scripts/check-size.sh

step "startup and memory budgets"
python3 scripts/measure-startup.py

printf '\n\033[1mall checks passed\033[0m\n'

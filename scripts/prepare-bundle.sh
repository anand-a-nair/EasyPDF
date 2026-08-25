#!/usr/bin/env bash
# Stages the worker binary and the PDFium library for Tauri's bundler.
#
# Two things have to end up inside the app package:
#
#   easypdf-worker  as a Tauri "external binary" (sidecar), which must be named
#                   with the target triple so the bundler picks the right one.
#   libpdfium       as a bundled resource.
#
# Getting either wrong fails in a specific and bad way: the app launches, finds
# no worker, and someone is tempted to "fix" it with an in-process fallback —
# which would silently discard the entire security model. There is no fallback
# (D-017), so this script failing is better than the app shipping broken.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${1:-release}"

triple="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "$triple" ]]; then
    echo "error: could not determine the host target triple" >&2
    exit 1
fi

case "$triple" in
    aarch64-apple-darwin)      pdfium_target="mac-arm64"  ; lib="libpdfium.dylib" ;;
    x86_64-apple-darwin)       pdfium_target="mac-x64"    ; lib="libpdfium.dylib" ;;
    x86_64-unknown-linux-gnu)  pdfium_target="linux-x64"  ; lib="libpdfium.so"    ;;
    x86_64-pc-windows-msvc)    pdfium_target="win-x64"    ; lib="pdfium.dll"      ;;
    *) echo "error: unsupported target $triple" >&2; exit 1 ;;
esac

extension=""
[[ "$triple" == *windows* ]] && extension=".exe"

binaries="$root/apps/desktop/src-tauri/binaries"
resources="$root/apps/desktop/src-tauri/resources"
mkdir -p "$binaries" "$resources"

# 1. The worker, named for the bundler.
worker="$root/target/$profile/easypdf-worker$extension"
if [[ ! -f "$worker" ]]; then
    echo "error: $worker not found — build it first:" >&2
    echo "  cargo build --$profile -p easypdf-worker" >&2
    exit 1
fi
cp "$worker" "$binaries/easypdf-worker-$triple$extension"
echo "staged worker: binaries/easypdf-worker-$triple$extension"

# 2. PDFium, verified present rather than assumed.
vendored="$root/vendor/pdfium/$pdfium_target/lib/$lib"
if [[ ! -f "$vendored" ]]; then
    echo "error: $vendored not found — run scripts/fetch-pdfium.sh first" >&2
    exit 1
fi
cp "$vendored" "$resources/$lib"
echo "staged pdfium: resources/$lib"

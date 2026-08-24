#!/usr/bin/env bash
# Downloads and verifies the pinned PDFium build.
#
# Required by TD-007. The hash check is not a formality: this binary is the
# project's most security-sensitive dependency and we did not build it
# ourselves. A mismatch aborts — it is never a warning, and there is no flag to
# skip it.
#
# Usage: scripts/fetch-pdfium.sh [target]
#   target defaults to the host platform.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$root/vendor/pdfium.lock"

detect_target() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Darwin) case "$arch" in
                    arm64) echo "mac-arm64" ;;
                    x86_64) echo "mac-x64" ;;
                    *) echo "unsupported macOS architecture: $arch" >&2; exit 1 ;;
                esac ;;
        Linux)  echo "linux-x64" ;;
        MINGW*|MSYS*|CYGWIN*) echo "win-x64" ;;
        *) echo "unsupported platform: $os" >&2; exit 1 ;;
    esac
}

read_field() {
    grep -E "^$1 *= *" "$manifest" | head -1 | sed -E 's/.*= *"([^"]*)".*/\1/'
}

read_hash() {
    grep -E "^$1 *= *" "$manifest" | head -1 | sed -E 's/.*= *"([^"]*)".*/\1/'
}

sha256() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

target="${1:-$(detect_target)}"
tag="$(read_field release_tag)"
version="$(read_field pdfium_version)"
expected="$(read_hash "$target")"

if [[ -z "$expected" ]]; then
    echo "error: no pinned hash for target '$target' in $manifest" >&2
    exit 1
fi

dest="$root/vendor/pdfium/$target"
stamp="$dest/.verified-$expected"

if [[ -f "$stamp" ]]; then
    echo "pdfium $version ($target) already present and verified"
    exit 0
fi

url="https://github.com/bblanchon/pdfium-binaries/releases/download/$tag/pdfium-$target.tgz"
archive="$(mktemp -t pdfium.XXXXXX).tgz"
trap 'rm -f "$archive"' EXIT

echo "fetching pdfium $version ($target)"
echo "  from $url"
curl -sSL --fail --max-time 300 -o "$archive" "$url"

actual="$(sha256 "$archive")"
if [[ "$actual" != "$expected" ]]; then
    echo "" >&2
    echo "SHA-256 MISMATCH — REFUSING TO INSTALL" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    echo "" >&2
    echo "The pinned artifact does not match what was downloaded. Either the" >&2
    echo "release was altered, or vendor/pdfium.lock is stale. Do not bypass" >&2
    echo "this check — investigate first." >&2
    exit 1
fi
echo "  sha256 verified: $actual"

rm -rf "$dest"
mkdir -p "$dest"
tar xzf "$archive" -C "$dest"

# Guard against a future manifest edit silently pulling a V8-enabled build,
# which would put a JavaScript engine into the process that handles untrusted
# documents. See ideas/07-security.md.
if grep -q "pdf_enable_v8 = true" "$dest/args.gn" 2>/dev/null; then
    echo "error: this build has V8 enabled; EasyPDF must never ship a PDF JavaScript engine" >&2
    rm -rf "$dest"
    exit 1
fi

touch "$stamp"
echo "installed to vendor/pdfium/$target"

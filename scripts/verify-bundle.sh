#!/usr/bin/env bash
# Checks that a built macOS bundle is self-contained.
#
# A bundle that links and launches can still be unable to open a document,
# because the worker or the PDF engine did not make it inside. That failure
# looks like a working app until someone picks a file, so it is worth an
# explicit check rather than trusting the build succeeded.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app="${1:-$root/target/release/bundle/macos/EasyPDF.app}"

if [[ ! -d "$app" ]]; then
    echo "error: $app not found — build it with: npm run tauri build -- --bundles app" >&2
    exit 1
fi

fail=0
require() {
    if [[ -f "$1" ]]; then
        echo "  ok      $(basename "$1")"
    else
        echo "  MISSING $1" >&2
        fail=1
    fi
}

echo "verifying $app"
require "$app/Contents/MacOS/easypdf-desktop"
require "$app/Contents/MacOS/easypdf-worker"
require "$app/Contents/Resources/resources/libpdfium.dylib"

if (( fail )); then
    echo "" >&2
    echo "The bundle is incomplete. It would launch and then refuse every" >&2
    echo "document — there is no in-process fallback by design (D-017)." >&2
    exit 1
fi

echo "bundle contents verified"
echo ""
echo "Note: this checks the files are present. Whether the worker can actually"
echo "load the engine is reported by its handshake (engine_available) and is"
echo "covered by the worker integration tests."

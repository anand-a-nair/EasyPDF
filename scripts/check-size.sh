#!/usr/bin/env bash
# Fails when the release binary exceeds the budget in
# ideas/04-performance-budget.md.
#
# Size creeps one dependency at a time; an automated gate is the only reliable
# defense. Raising the ceiling is a decision — record it per .claude/RULES.md.
set -euo pipefail

# Budget for the bare executable, before bundling and before PDFium is
# vendored. The 15 MB installer budget is checked separately once bundling
# works on all three platforms.
BUDGET_BYTES=$((8 * 1024 * 1024))

binary="${1:-target/release/easypdf-desktop}"

if [[ ! -f "$binary" ]]; then
    echo "error: $binary not found — run a release build first" >&2
    exit 1
fi

size=$(wc -c < "$binary" | tr -d ' ')
printf 'binary: %s\nsize:   %s bytes (%.2f MB)\nbudget: %s bytes (%.2f MB)\n' \
    "$binary" "$size" "$(echo "$size" | awk '{print $1/1048576}')" \
    "$BUDGET_BYTES" "$(echo "$BUDGET_BYTES" | awk '{print $1/1048576}')"

if (( size > BUDGET_BYTES )); then
    echo "FAIL: over budget by $(( size - BUDGET_BYTES )) bytes" >&2
    echo "Either find an offsetting saving or record a decision to raise it." >&2
    exit 1
fi

echo "PASS"

# The installer is the number that actually matters to a user; the bare
# executable above is only a proxy that can be checked without bundling.
# Checked when present rather than required, because most builds do not bundle.
INSTALLER_BUDGET_BYTES=$((15 * 1024 * 1024))

installer=""
for candidate in \
    target/release/bundle/dmg/*.dmg \
    target/release/bundle/msi/*.msi \
    target/release/bundle/nsis/*.exe \
    target/release/bundle/appimage/*.AppImage \
    target/release/bundle/deb/*.deb
do
    [[ -f "$candidate" ]] && installer="$candidate" && break
done

if [[ -n "$installer" ]]; then
    installer_size=$(wc -c < "$installer" | tr -d ' ')
    printf '\ninstaller: %s\nsize:      %s bytes (%.2f MB)\nbudget:    %.2f MB\n' \
        "$(basename "$installer")" "$installer_size" \
        "$(echo "$installer_size" | awk '{print $1/1048576}')" \
        "$(echo "$INSTALLER_BUDGET_BYTES" | awk '{print $1/1048576}')"

    if (( installer_size > INSTALLER_BUDGET_BYTES )); then
        echo "FAIL: installer over budget by $(( installer_size - INSTALLER_BUDGET_BYTES )) bytes" >&2
        exit 1
    fi
    echo "PASS"
else
    echo ""
    echo "note: no installer found; skipped the installer budget check"
fi

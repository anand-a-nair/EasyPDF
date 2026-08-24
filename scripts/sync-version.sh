#!/usr/bin/env bash
# Propagates .version to every manifest that carries a version number.
#
# .version is the single source of truth (see ideas/12-build-and-release.md).
# Run after bumping it; CI verifies the manifests have not drifted.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(tr -d '[:space:]' < "$root/.version")"

# tauri.conf.json rejects semver pre-release suffixes, so strip them there.
bare_version="${version%%-*}"

echo "syncing version: $version (tauri: $bare_version)"

# Cargo workspace
perl -0pi -e "s/^version = \"[^\"]*\"/version = \"$version\"/m" \
    "$root/Cargo.toml"

# Frontend package
perl -0pi -e "s/(\"version\":\s*)\"[^\"]*\"/\${1}\"$version\"/" \
    "$root/apps/desktop/package.json"

# Tauri bundle metadata
perl -0pi -e "s/(\"version\":\s*)\"[^\"]*\"/\${1}\"$bare_version\"/" \
    "$root/apps/desktop/src-tauri/tauri.conf.json"

echo "done"

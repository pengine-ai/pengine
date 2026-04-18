#!/usr/bin/env bash
# Stage the binaries produced by tauri-action into a flat directory so they
# can be uploaded as GitHub Actions build artifacts and attested.
#
# Env: ARTIFACT_PATHS (JSON array from tauri-action's artifactPaths output).
# Output: dir=<staging path> appended to $GITHUB_OUTPUT.
set -euo pipefail

: "${ARTIFACT_PATHS:?}"

staging="${GITHUB_WORKSPACE:-$PWD}/dist/release"
rm -rf "$staging"
mkdir -p "$staging"

while IFS= read -r src; do
  # Strip stray CR — on windows-latest the env var can arrive CRLF-terminated.
  src="${src%$'\r'}"
  [ -z "$src" ] && continue
  # tauri-action lists the .app bundle (a directory) alongside the .dmg on
  # macOS; skip anything that isn't a regular file.
  if [ ! -f "$src" ]; then
    echo "Skipping non-file artifact: $src" >&2
    continue
  fi
  cp "$src" "$staging/"
done < <(printf '%s' "$ARTIFACT_PATHS" | jq -r '.[]')

count=$(find "$staging" -maxdepth 1 -type f | wc -l | tr -d ' ')
if [ "$count" -eq 0 ]; then
  echo "No binary artifacts produced by tauri-action." >&2
  exit 1
fi

echo "dir=$staging" >> "$GITHUB_OUTPUT"
echo "Staged $count file(s) in $staging"

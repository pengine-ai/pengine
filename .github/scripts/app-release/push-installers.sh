#!/usr/bin/env bash
# Push the installers produced by tauri-action to GHCR as an OCI artifact so
# they show up under the org's linked artifacts page.
# Env: ARTIFACT_PATHS (JSON array from tauri-action), TAG, SLUG (macos|linux|
# windows), OWNER, REPO, REVISION, GHCR_TOKEN.
set -euo pipefail

: "${ARTIFACT_PATHS:?}"
: "${TAG:?}"
: "${SLUG:?}"
: "${OWNER:?}"
: "${REPO:?}"
: "${REVISION:?}"
: "${GHCR_TOKEN:?}"

owner_lower=$(printf '%s' "$OWNER" | tr '[:upper:]' '[:lower:]')
version="${TAG#v}"
image="ghcr.io/${owner_lower}/pengine-installer-${SLUG}:${version}"

# Stage artifacts into a scratch dir so oras records flat basenames rather
# than the long tauri-action build paths.
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT

files=()
while IFS= read -r src; do
  [ -z "$src" ] && continue
  cp "$src" "$staging/"
  files+=("$(basename "$src"):application/octet-stream")
done < <(printf '%s' "$ARTIFACT_PATHS" | jq -r '.[]')

if [ ${#files[@]} -eq 0 ]; then
  echo "No installer artifacts produced by tauri-action; skipping GHCR push." >&2
  exit 0
fi

printf '%s' "$GHCR_TOKEN" | oras login ghcr.io -u "$OWNER" --password-stdin

echo "Pushing ${#files[@]} file(s) to ${image}"
( cd "$staging" && oras push "$image" \
    --artifact-type "application/vnd.pengine.installer.v1" \
    --annotation "org.opencontainers.image.source=https://github.com/${REPO}" \
    --annotation "org.opencontainers.image.revision=${REVISION}" \
    --annotation "org.opencontainers.image.version=${version}" \
    --annotation "org.opencontainers.image.title=pengine installers (${SLUG})" \
    "${files[@]}" )

{
  echo "## pengine-installer-${SLUG}"
  echo
  echo "- Image: \`${image}\`"
  echo "- Files:"
  for f in "${files[@]}"; do
    echo "  - \`${f%%:*}\`"
  done
} >> "${GITHUB_STEP_SUMMARY:-/dev/null}"

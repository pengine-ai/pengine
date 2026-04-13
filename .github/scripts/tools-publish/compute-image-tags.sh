#!/usr/bin/env bash
# Writes multiline "tags" to GITHUB_OUTPUT for docker/build-push-action.
# Env: IMAGE, VERSION, REF_TYPE (github.ref_type: branch|tag), GITHUB_REF.
set -euo pipefail

TAGS="${IMAGE}:${VERSION}"
LATEST=false
if [[ "${REF_TYPE}" == "branch" && "${GITHUB_REF}" == "refs/heads/main" ]]; then
  LATEST=true
elif [[ "${REF_TYPE}" == "tag" ]]; then
  T="${GITHUB_REF#refs/tags/}"
  T="${T#v}"
  if [[ "$T" == "$VERSION" ]]; then
    LATEST=true
  fi
fi
if [[ "$LATEST" == "true" ]]; then
  TAGS="${TAGS}"$'\n'"${IMAGE}:latest"
fi
{
  echo 'tags<<EOF'
  echo "$TAGS"
  echo 'EOF'
} >> "$GITHUB_OUTPUT"

#!/usr/bin/env bash
# Writes image, version, npm_pkg, npm_ver to GITHUB_OUTPUT for one tool slug.
# Usage: TOOL_SLUG=file-manager (env) or first argument.
set -euo pipefail

REGISTRY="tools/mcp-tools.json"
SUFFIX="${1:-${TOOL_SLUG:-}}"
if [[ -z "$SUFFIX" ]]; then
  echo "::error::TOOL_SLUG (or \$1) is required" >&2
  exit 1
fi

IMAGE=$(jq -r --arg s "$SUFFIX" '.tools[] | select(.id | endswith("/" + $s)) | .image' "$REGISTRY")
VERSION=$(jq -r --arg s "$SUFFIX" '.tools[] | select(.id | endswith("/" + $s)) | .current' "$REGISTRY")
echo "image=$IMAGE" >> "$GITHUB_OUTPUT"
echo "version=$VERSION" >> "$GITHUB_OUTPUT"

PKG=$(jq -r --arg s "$SUFFIX" '.tools[] | select(.id | endswith("/" + $s)) | .upstream_mcp_npm.package // ""' "$REGISTRY")
NPM_VER=$(jq -r --arg s "$SUFFIX" '.tools[] | select(.id | endswith("/" + $s)) | .upstream_mcp_npm.version // ""' "$REGISTRY")
if [[ -z "$PKG" || -z "$NPM_VER" ]]; then
  echo "::error::${REGISTRY}: tool '${SUFFIX}' must define non-empty upstream_mcp_npm.package and upstream_mcp_npm.version" >&2
  exit 1
fi
echo "npm_pkg=$PKG" >> "$GITHUB_OUTPUT"
echo "npm_ver=$NPM_VER" >> "$GITHUB_OUTPUT"

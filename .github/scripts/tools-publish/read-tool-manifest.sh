#!/usr/bin/env bash
# Writes image, version, and multiline build_args to GITHUB_OUTPUT for one tool slug.
# Each tool must define either upstream_mcp_npm or upstream_mcp_pypi (not both).
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

npm_pkg=$(jq -r --arg s "$SUFFIX" '.tools[] | select(.id | endswith("/" + $s)) | .upstream_mcp_npm.package // ""' "$REGISTRY")
npm_ver=$(jq -r --arg s "$SUFFIX" '.tools[] | select(.id | endswith("/" + $s)) | .upstream_mcp_npm.version // ""' "$REGISTRY")
pypi_pkg=$(jq -r --arg s "$SUFFIX" '.tools[] | select(.id | endswith("/" + $s)) | .upstream_mcp_pypi.package // ""' "$REGISTRY")
pypi_ver=$(jq -r --arg s "$SUFFIX" '.tools[] | select(.id | endswith("/" + $s)) | .upstream_mcp_pypi.version // ""' "$REGISTRY")

has_npm=0
[[ -n "$npm_pkg" && -n "$npm_ver" ]] && has_npm=1
has_pypi=0
[[ -n "$pypi_pkg" && -n "$pypi_ver" ]] && has_pypi=1

if [[ "$has_npm" -eq 1 && "$has_pypi" -eq 1 ]]; then
  echo "::error::${REGISTRY}: tool '${SUFFIX}' must not set both upstream_mcp_npm and upstream_mcp_pypi" >&2
  exit 1
fi
if [[ "$has_npm" -eq 0 && "$has_pypi" -eq 0 ]]; then
  echo "::error::${REGISTRY}: tool '${SUFFIX}' must define upstream_mcp_npm or upstream_mcp_pypi" >&2
  exit 1
fi

{
  echo 'build_args<<BUILD_ARGS_EOF'
  if [[ "$has_npm" -eq 1 ]]; then
    echo "UPSTREAM_MCP_NPM_PACKAGE=$npm_pkg"
    echo "UPSTREAM_MCP_NPM_VERSION=$npm_ver"
  fi
  if [[ "$has_pypi" -eq 1 ]]; then
    echo "UPSTREAM_MCP_PYPI_PACKAGE=$pypi_pkg"
    echo "UPSTREAM_MCP_PYPI_VERSION=$pypi_ver"
  fi
  echo 'BUILD_ARGS_EOF'
} >> "$GITHUB_OUTPUT"

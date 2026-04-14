#!/usr/bin/env bash
# update-upstream.sh — check npm / PyPI for newer upstream MCP packages
# and bump versions in mcp-tools.json (like `npm update` for tool images).
#
# Usage:
#   ./tools/update-upstream.sh              # check all tools
#   ./tools/update-upstream.sh file-manager # check one tool
#
# After running, commit the changes and push — CI builds only the affected images.

set -euo pipefail

TOOLS_FILE="$(cd "$(dirname "$0")" && pwd)/mcp-tools.json"

if ! command -v jq &>/dev/null; then
  echo "error: jq is required (brew install jq)" >&2
  exit 1
fi

if ! command -v npm &>/dev/null; then
  echo "error: npm is required" >&2
  exit 1
fi

if ! command -v curl &>/dev/null; then
  echo "error: curl is required" >&2
  exit 1
fi

if [[ ! -f "$TOOLS_FILE" ]]; then
  echo "error: $TOOLS_FILE not found" >&2
  exit 1
fi

FILTER="${1:-}"
CHANGED=0

tool_count=$(jq '.tools | length' "$TOOLS_FILE")

bump_tool() {
  local idx="$1"
  local kind="$2"
  local new_upstream_ver="$3"
  local current_tool
  local new_tool_version
  current_tool=$(jq -r ".tools[$idx].current" "$TOOLS_FILE")
  IFS='.' read -r major minor patch <<< "$current_tool"
  new_tool_version="${major}.${minor}.$((patch + 1))"
  echo "[$slug] bumping tool version: $current_tool → $new_tool_version"
  tmp=$(mktemp)
  if [[ "$kind" == "npm" ]]; then
    jq --arg idx "$idx" \
       --arg npm_ver "$new_upstream_ver" \
       --arg tool_ver "$new_tool_version" \
       --arg now "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '
      .tools[($idx | tonumber)].upstream_mcp_npm.version = $npm_ver |
      .tools[($idx | tonumber)].current = $tool_ver |
      .tools[($idx | tonumber)].versions += [{
        version: $tool_ver,
        digest: "sha256:placeholder",
        released_at: $now,
        yanked: false,
        revoked: false,
        security: false
      }]
    ' "$TOOLS_FILE" > "$tmp" && mv "$tmp" "$TOOLS_FILE"
  else
    jq --arg idx "$idx" \
       --arg pypi_ver "$new_upstream_ver" \
       --arg tool_ver "$new_tool_version" \
       --arg now "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '
      .tools[($idx | tonumber)].upstream_mcp_pypi.version = $pypi_ver |
      .tools[($idx | tonumber)].current = $tool_ver |
      .tools[($idx | tonumber)].versions += [{
        version: $tool_ver,
        digest: "sha256:placeholder",
        released_at: $now,
        yanked: false,
        revoked: false,
        security: false
      }]
    ' "$TOOLS_FILE" > "$tmp" && mv "$tmp" "$TOOLS_FILE"
  fi
  CHANGED=$((CHANGED + 1))
}

for i in $(seq 0 $((tool_count - 1))); do
  tool_id=$(jq -r ".tools[$i].id" "$TOOLS_FILE")
  slug=$(echo "$tool_id" | cut -d/ -f2)

  if [[ -n "$FILTER" && "$slug" != "$FILTER" ]]; then
    continue
  fi

  npm_pkg=$(jq -r ".tools[$i].upstream_mcp_npm.package // empty" "$TOOLS_FILE")
  pypi_pkg=$(jq -r ".tools[$i].upstream_mcp_pypi.package // empty" "$TOOLS_FILE")

  if [[ -n "$npm_pkg" ]]; then
    current_npm=$(jq -r ".tools[$i].upstream_mcp_npm.version" "$TOOLS_FILE")
    echo -n "[$slug] npm $npm_pkg@$current_npm … "
    latest_npm=$(npm view "$npm_pkg" version 2>/dev/null || echo "")
    if [[ -z "$latest_npm" ]]; then
      echo "failed to query npm registry"
      continue
    fi
    if [[ "$latest_npm" == "$current_npm" ]]; then
      echo "up to date ($current_npm)"
      continue
    fi
    echo "new version: $current_npm → $latest_npm"
    bump_tool "$i" "npm" "$latest_npm"
    continue
  fi

  if [[ -n "$pypi_pkg" ]]; then
    current_pypi=$(jq -r ".tools[$i].upstream_mcp_pypi.version" "$TOOLS_FILE")
    echo -n "[$slug] PyPI $pypi_pkg@$current_pypi … "
    latest_pypi=$(curl -fsSL "https://pypi.org/pypi/${pypi_pkg}/json" | jq -r '.info.version' 2>/dev/null || echo "")
    if [[ -z "$latest_pypi" || "$latest_pypi" == "null" ]]; then
      echo "failed to query PyPI"
      continue
    fi
    if [[ "$latest_pypi" == "$current_pypi" ]]; then
      echo "up to date ($current_pypi)"
      continue
    fi
    echo "new version: $current_pypi → $latest_pypi"
    bump_tool "$i" "pypi" "$latest_pypi"
    continue
  fi

  echo "[$slug] no upstream_mcp_npm or upstream_mcp_pypi — skipped"
done

echo ""
if [[ $CHANGED -gt 0 ]]; then
  echo "$CHANGED tool(s) updated. Review the diff, then commit and push:"
  echo "  git add tools/mcp-tools.json src-tauri/src/modules/tool_engine/tools.json"
  echo "  git commit -m 'chore: bump upstream MCP packages'"
  echo "  git push"
  echo ""
  echo "CI will build tools whose catalog entry changed."
else
  echo "All tools are up to date."
fi

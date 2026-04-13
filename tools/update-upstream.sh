#!/usr/bin/env bash
# update-upstream.sh — check npm registry for newer upstream MCP packages
# and bump versions in mcp-tools.json (like `npm update` for tool images).
#
# Usage:
#   ./tools/update-upstream.sh              # check all tools
#   ./tools/update-upstream.sh file-manager # check one tool
#
# What it does:
#   1. For each tool with upstream_mcp_npm, query npm for the latest version
#   2. If newer, update mcp-tools.json (upstream version + tool patch bump)
#   3. Print a summary of what changed
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

if [[ ! -f "$TOOLS_FILE" ]]; then
  echo "error: $TOOLS_FILE not found" >&2
  exit 1
fi

FILTER="${1:-}"
CHANGED=0

tool_count=$(jq '.tools | length' "$TOOLS_FILE")

for i in $(seq 0 $((tool_count - 1))); do
  tool_id=$(jq -r ".tools[$i].id" "$TOOLS_FILE")
  slug=$(echo "$tool_id" | cut -d/ -f2)

  # Skip if user asked for a specific tool
  if [[ -n "$FILTER" && "$slug" != "$FILTER" ]]; then
    continue
  fi

  npm_pkg=$(jq -r ".tools[$i].upstream_mcp_npm.package // empty" "$TOOLS_FILE")
  if [[ -z "$npm_pkg" ]]; then
    echo "[$slug] no upstream_mcp_npm — skipped"
    continue
  fi

  current_npm=$(jq -r ".tools[$i].upstream_mcp_npm.version" "$TOOLS_FILE")
  current_tool=$(jq -r ".tools[$i].current" "$TOOLS_FILE")

  echo -n "[$slug] checking $npm_pkg@$current_npm … "

  latest_npm=$(npm view "$npm_pkg" version 2>/dev/null || echo "")
  if [[ -z "$latest_npm" ]]; then
    echo "failed to query npm registry"
    continue
  fi

  if [[ "$latest_npm" == "$current_npm" ]]; then
    echo "up to date ($current_npm)"
    continue
  fi

  echo "new version available: $current_npm → $latest_npm"

  # Bump the tool's patch version (0.1.0 → 0.1.1, 0.2.3 → 0.2.4)
  IFS='.' read -r major minor patch <<< "$current_tool"
  new_tool_version="${major}.${minor}.$((patch + 1))"

  echo "[$slug] bumping tool version: $current_tool → $new_tool_version"

  # Update mcp-tools.json in place
  tmp=$(mktemp)
  jq --arg idx "$i" \
     --arg npm_ver "$latest_npm" \
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

  CHANGED=$((CHANGED + 1))
done

echo ""
if [[ $CHANGED -gt 0 ]]; then
  echo "$CHANGED tool(s) updated. Review the diff, then commit and push:"
  echo "  git add tools/mcp-tools.json"
  echo "  git commit -m 'chore: bump upstream MCP packages'"
  echo "  git push"
  echo ""
  echo "CI will build only the tools whose version changed."
else
  echo "All tools are up to date."
fi

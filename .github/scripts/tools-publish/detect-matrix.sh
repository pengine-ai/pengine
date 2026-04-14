#!/usr/bin/env bash
# Outputs: skip, matrix (JSON array of slugs) to GITHUB_OUTPUT.
# Env: GITHUB_EVENT_NAME, INPUT_TOOL (workflow_dispatch tool input; empty on push).
set -euo pipefail

REGISTRY="tools/mcp-tools.json"

all=""
while IFS= read -r slug; do
  [ -z "$slug" ] && continue
  [ -f "tools/${slug}/Dockerfile" ] && all="$all $slug"
done < <(jq -r '.tools[].id | split("/")[1]' "$REGISTRY")

slugs=""
if [[ "${GITHUB_EVENT_NAME}" == "workflow_dispatch" ]]; then
  input="${INPUT_TOOL:-}"
  if [[ "$input" == "all" || -z "$input" ]]; then
    slugs="$all"
  else
    slugs="$input"
  fi
else
  changed=$(git diff --name-only HEAD~1 HEAD 2>/dev/null || true)
  for s in $all; do
    if echo "$changed" | grep -q "^tools/${s}/"; then
      slugs="$slugs $s"
      continue
    fi
    if echo "$changed" | grep -q "^tools/mcp-tools.json$"; then
      old_blob=$(git show HEAD~1:tools/mcp-tools.json 2>/dev/null \
        | jq -c --arg s "$s" '.tools[]? | select(.id | endswith("/" + $s))' 2>/dev/null || echo "")
      new_blob=$(jq -c --arg s "$s" '.tools[] | select(.id | endswith("/" + $s))' "$REGISTRY")
      if [[ "$old_blob" != "$new_blob" ]]; then
        slugs="$slugs $s"
      fi
    fi
  done
fi

json="["
for s in $slugs; do
  [ -f "tools/${s}/Dockerfile" ] && json="$json\"$s\","
done
json="${json%,}]"

if [[ "$json" == "[]" ]]; then
  echo "skip=true" >> "$GITHUB_OUTPUT"
else
  echo "skip=false" >> "$GITHUB_OUTPUT"
fi
echo "matrix=$json" >> "$GITHUB_OUTPUT"
echo "Tools: $json"

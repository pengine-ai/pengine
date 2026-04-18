#!/usr/bin/env bash
# Pull image by digest and run one MCP initialize JSON-RPC round-trip.
# Env: IMAGE_WITH_DIGEST (e.g. ghcr.io/org/img@sha256:...). Optional TOOL_SLUG for argv quirks.
set -euo pipefail

if [[ -z "${IMAGE_WITH_DIGEST:-}" ]]; then
  echo "::error::IMAGE_WITH_DIGEST is required" >&2
  exit 1
fi

docker pull "$IMAGE_WITH_DIGEST"

# Filesystem MCP expects at least one allowed root on argv; others ignore extra args.
extra=()
if [[ "${TOOL_SLUG:-}" == "file-manager" ]]; then
  extra=(/tmp)
fi

# Brave Search MCP refuses to start with an empty key; init handshake does not call the API.
docker_run_env=()
if [[ "${TOOL_SLUG:-}" == "brave-search" ]]; then
  docker_run_env=(-e "BRAVE_API_KEY=${SMOKE_BRAVE_API_KEY:-smoke-ci-placeholder}")
fi

RESP=$(echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.1"}}}' \
  | timeout 15 docker run --rm -i --network=none "${docker_run_env[@]}" "$IMAGE_WITH_DIGEST" "${extra[@]}" \
  | head -1)
echo "$RESP" | jq -e '.result.serverInfo' > /dev/null \
  || { echo "::error::MCP init failed: $RESP"; exit 1; }

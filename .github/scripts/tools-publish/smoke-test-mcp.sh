#!/usr/bin/env bash
# Pull image by digest and run one MCP initialize JSON-RPC round-trip.
# Env: IMAGE_WITH_DIGEST (e.g. ghcr.io/org/img@sha256:...).
set -euo pipefail

if [[ -z "${IMAGE_WITH_DIGEST:-}" ]]; then
  echo "::error::IMAGE_WITH_DIGEST is required" >&2
  exit 1
fi

docker pull "$IMAGE_WITH_DIGEST"
RESP=$(echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.1"}}}' \
  | timeout 15 docker run --rm -i --network=none "$IMAGE_WITH_DIGEST" /tmp \
  | head -1)
echo "$RESP" | jq -e '.result.serverInfo' > /dev/null \
  || { echo "::error::MCP init failed: $RESP"; exit 1; }

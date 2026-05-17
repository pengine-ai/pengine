#!/usr/bin/env bash
# Run probe.mjs against the File Manager Docker image (same layout as Pengine Tool Engine).
#
#   ./tools/build-local-images.sh    # builds ghcr.io/pengine-ai/pengine-file-manager:0.1.0
#   ./tools/mcp-probe-filemanager/probe-in-image.sh /path/to/project
#
# Uses podman if installed, else docker (override with PENGINE_CONTAINER_RUNTIME).

set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
export MCP_PROBE_IN_CONTAINER=1
if [[ -z "${PENGINE_CONTAINER_RUNTIME:-}" ]]; then
  if command -v podman &>/dev/null; then
    export PENGINE_CONTAINER_RUNTIME=podman
  elif command -v docker &>/dev/null; then
    export PENGINE_CONTAINER_RUNTIME=docker
  else
    echo "install podman or docker, or set PENGINE_CONTAINER_RUNTIME" >&2
    exit 1
  fi
fi
exec node "$DIR/probe.mjs" "${1:-.}"

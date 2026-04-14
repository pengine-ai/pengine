#!/usr/bin/env bash
# Build every tool image from tools/<slug>/Dockerfile and tag as in tools/mcp-tools.json
# (image:current) so the dashboard / podman run resolve them without GHCR.
#
# Uses **podman** when available (or set PENGINE_CONTAINER_RUNTIME explicitly, e.g. `docker`).
#
# Usage (repo root):
#   ./tools/build-local-images.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REG="${ROOT}/tools/mcp-tools.json"

if [[ -n "${PENGINE_CONTAINER_RUNTIME:-}" ]]; then
  RUNTIME="${PENGINE_CONTAINER_RUNTIME}"
elif command -v podman &>/dev/null; then
  RUNTIME=podman
elif command -v docker &>/dev/null; then
  RUNTIME=docker
else
  echo "error: install podman or docker, or set PENGINE_CONTAINER_RUNTIME" >&2
  exit 1
fi
echo "Using container runtime: ${RUNTIME}"

# Prefer host CPU arch so base images match (avoids linux/amd64 on Apple Silicon, etc.).
BUILD_PLATFORM="${PENGINE_CONTAINER_PLATFORM:-}"
if [[ -z "$BUILD_PLATFORM" ]]; then
  case "$(uname -m)" in
    arm64|aarch64) BUILD_PLATFORM=linux/arm64 ;;
    x86_64|amd64) BUILD_PLATFORM=linux/amd64 ;;
  esac
fi
PLATFORM_ARGS=()
if [[ -n "${BUILD_PLATFORM:-}" ]]; then
  PLATFORM_ARGS=(--platform "${BUILD_PLATFORM}")
  echo "Using --platform ${BUILD_PLATFORM} (set PENGINE_CONTAINER_PLATFORM= to disable)"
fi

# Space-separated (avoid @tsv + split quirks in some jq versions).
while read -r slug image current; do
  [[ -z "$slug" ]] && continue
  ctx="${ROOT}/tools/${slug}"
  df="${ctx}/Dockerfile"
  if [[ ! -f "$df" ]]; then
    echo "skip $slug: no $df" >&2
    continue
  fi
  tag="${image}:${current}"
  echo "=== build $slug -> $tag ==="
  "${RUNTIME}" build "${PLATFORM_ARGS[@]}" -f "$df" -t "$tag" "$ctx"
done < <(jq -r '.tools[] | "\(.id | split("/")[1]) \(.image) \(.current)"' "$REG")

echo "Done. Images tagged as <image from mcp-tools.json>:<current>."

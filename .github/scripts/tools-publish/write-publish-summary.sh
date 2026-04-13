#!/usr/bin/env bash
# Appends a job summary section for one published tool.
# Env: TOOL_SLUG, TOOL_VERSION, IMAGE_REF, IMAGE_DIGEST, RUNNER_ARCH (optional; GitHub runner.arch).
set -euo pipefail

RUNNER_LINE=""
if [[ -n "${RUNNER_ARCH:-}" ]]; then
  RUNNER_LINE="- **CI host arch:** \`${RUNNER_ARCH}\` — smoke test pulled the matching layer from the **single** multi-arch manifest below (your machine does the same)."
fi

cat >> "$GITHUB_STEP_SUMMARY" <<EOF
### ${TOOL_SLUG} v${TOOL_VERSION}
- **Image (multi-arch index):** \`${IMAGE_REF}\`
${RUNNER_LINE}
- **Architectures in manifest:** linux/amd64, linux/arm64
- **Signed:** cosign keyless

Digest for mcp-tools.json (one value for all platforms):
\`\`\`
${IMAGE_DIGEST}
\`\`\`

Staging tags \`:${TOOL_VERSION}-ci-amd64-<run>\` / \`:${TOOL_VERSION}-ci-arm64-<run>\` may appear in the package; **use \`:${TOOL_VERSION}\` or the digest above**—not the \`-ci-*\` tags.
EOF

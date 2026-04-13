#!/usr/bin/env bash
# Appends a job summary section for one published tool.
# Env: TOOL_SLUG, TOOL_VERSION, IMAGE_REF, IMAGE_DIGEST.
set -euo pipefail

cat >> "$GITHUB_STEP_SUMMARY" <<EOF
### ${TOOL_SLUG} v${TOOL_VERSION}
- **Image:** \`${IMAGE_REF}\`
- **Platforms:** linux/amd64, linux/arm64
- **Signed:** cosign keyless

Digest for mcp-tools.json:
\`\`\`
${IMAGE_DIGEST}
\`\`\`
EOF

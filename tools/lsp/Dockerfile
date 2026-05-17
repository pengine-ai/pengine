# cclsp bridges MCP to one or more LSP servers. Image bakes in TypeScript,
# Python, Go, and Rust LSPs and a default config. Users can derive from this
# image and edit `/mcp/cclsp.json` for additional languages.
#
# Upstream MCP npm package + version: CI passes --build-arg from tools/mcp-tools.json (`upstream_mcp_npm`).

# Stage 1 — build `gopls` from source (no upstream binary release exists; `go install` is the
# only supported install path). Kept in its own stage so the Go toolchain is not in the final image.
ARG GOLANG_VERSION=1.22-bookworm
FROM golang:${GOLANG_VERSION} AS gopls-builder
ARG GOPLS_VERSION=latest
ENV GOTOOLCHAIN=auto
RUN GOBIN=/out go install golang.org/x/tools/gopls@${GOPLS_VERSION}

# `node:22-bookworm` (non-slim) ships `useradd` / `groupadd` out of the box; the
# `-slim` variant keeps the `passwd` dpkg record but strips its binaries.
FROM node:22-bookworm
ARG TARGETARCH
ARG UPSTREAM_MCP_NPM_PACKAGE=cclsp
ARG UPSTREAM_MCP_NPM_VERSION=0.7.0
ARG TS_LSP_VERSION=5.1.3
ARG PYLSP_VERSION=1.14.0

ENV PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PIP_BREAK_SYSTEM_PACKAGES=1 \
    NODE_ENV=production \
    CCLSP_CONFIG_PATH=/mcp/cclsp.json \
    PATH=/opt/lsp:/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        python3 python3-pip ca-certificates curl xz-utils passwd \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -r -u 10001 -m -d /home/mcp mcp \
    && mkdir -p /mcp /opt/lsp \
    && chown mcp:mcp /mcp /opt/lsp

# rust-analyzer official binary release. The `latest/download/` URL always
# resolves to the most recent tag; rebuild the image to pick up upstream updates.
RUN ARCH="${TARGETARCH:-$(dpkg --print-architecture)}" \
    && case "${ARCH}" in \
         amd64) RA_TARGET=x86_64-unknown-linux-gnu ;; \
         arm64) RA_TARGET=aarch64-unknown-linux-gnu ;; \
         *) echo "unsupported arch: ${ARCH}" >&2; exit 1 ;; \
       esac \
    && curl -fsSL "https://github.com/rust-lang/rust-analyzer/releases/latest/download/rust-analyzer-${RA_TARGET}.gz" \
         | gunzip > /opt/lsp/rust-analyzer \
    && chmod +x /opt/lsp/rust-analyzer

COPY --from=gopls-builder /out/gopls /opt/lsp/gopls

RUN pip install --no-cache-dir "python-lsp-server==${PYLSP_VERSION}"

WORKDIR /mcp
RUN npm install --omit=dev --prefix /mcp \
        "${UPSTREAM_MCP_NPM_PACKAGE}@${UPSTREAM_MCP_NPM_VERSION}" \
        "typescript-language-server@${TS_LSP_VERSION}" \
        "typescript" \
    && npm cache clean --force \
    && rm -rf /root/.npm \
    && chown -R mcp:mcp /mcp

COPY cclsp.json /mcp/cclsp.json

USER mcp
ENTRYPOINT ["node", "/mcp/node_modules/cclsp/dist/index.js"]

# Upstream MCP npm package + version: CI passes --build-arg from tools/mcp-tools.json (`upstream_mcp_npm`).
FROM node:22-alpine
ARG UPSTREAM_MCP_NPM_PACKAGE=@mako10k/mcp-shell-server
ARG UPSTREAM_MCP_NPM_VERSION=2.7.1
# Shell server shells out to the system shell. `bash` covers heredocs and richer
# scripts; `coreutils` replaces busybox stubs whose flags differ from POSIX
# variants (e.g. `head -n`, `du -h`).
RUN apk add --no-cache bash coreutils
RUN addgroup -S mcp && adduser -S -G mcp -H mcp
WORKDIR /mcp
# `@mako10k/mcp-shell-server` pulls `node-pty` for the terminal_* tools.
# `node-pty` has no prebuilt binary for `linux-arm64`, so npm rebuilds it from
# source with node-gyp, which needs python3 + a C++ toolchain. Install those as
# a `.build-deps` virtual package and drop it after install to keep the runtime
# image slim.
RUN apk add --no-cache --virtual .build-deps python3 make g++ \
    && npm install --omit=dev --prefix /mcp "${UPSTREAM_MCP_NPM_PACKAGE}@${UPSTREAM_MCP_NPM_VERSION}" \
    && npm cache clean --force \
    && rm -rf /root/.npm \
    && chown -R mcp:mcp /mcp \
    && apk del .build-deps
USER mcp
ENV NODE_ENV=production
ENTRYPOINT ["node", "/mcp/node_modules/@mako10k/mcp-shell-server/dist/index.js"]

# Upstream MCP npm package + version: defaults match the tool manifest; CI passes --build-arg from
# tools/file-manager/pengine-tool.json (`upstream_mcp_npm`) so that file is the source of truth.
FROM node:22-alpine
ARG UPSTREAM_MCP_NPM_PACKAGE=@modelcontextprotocol/server-filesystem
ARG UPSTREAM_MCP_NPM_VERSION=2026.1.14
RUN addgroup -S mcp && adduser -S -G mcp -H mcp
WORKDIR /mcp
RUN npm install --omit=dev --prefix /mcp "${UPSTREAM_MCP_NPM_PACKAGE}@${UPSTREAM_MCP_NPM_VERSION}" \
    && npm cache clean --force \
    && rm -rf /root/.npm \
    && chown -R mcp:mcp /mcp
USER mcp
ENV NODE_ENV=production
ENTRYPOINT ["node", "/mcp/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"]

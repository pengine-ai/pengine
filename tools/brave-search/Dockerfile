# Upstream MCP npm package + version: CI passes --build-arg from tools/mcp-tools.json (`upstream_mcp_npm`).
FROM node:22-alpine
ARG UPSTREAM_MCP_NPM_PACKAGE=@brave/brave-search-mcp-server
ARG UPSTREAM_MCP_NPM_VERSION=2.0.76
RUN addgroup -S mcp && adduser -S -G mcp -H mcp
WORKDIR /mcp
RUN npm install --omit=dev --prefix /mcp "${UPSTREAM_MCP_NPM_PACKAGE}@${UPSTREAM_MCP_NPM_VERSION}" \
    && npm cache clean --force \
    && rm -rf /root/.npm \
    && chown -R mcp:mcp /mcp
USER mcp
ENV NODE_ENV=production
ENTRYPOINT ["node", "/mcp/node_modules/@brave/brave-search-mcp-server/dist/index.js"]

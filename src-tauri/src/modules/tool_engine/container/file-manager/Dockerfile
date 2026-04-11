FROM node:22-alpine
RUN addgroup -S mcp && adduser -S -G mcp -H mcp
WORKDIR /mcp
COPY package.json package-lock.json ./
RUN npm ci --omit=dev \
    && npm cache clean --force \
    && rm -rf /root/.npm \
    && chown -R mcp:mcp /mcp
USER mcp
ENV NODE_ENV=production
ENTRYPOINT ["node", "/mcp/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"]

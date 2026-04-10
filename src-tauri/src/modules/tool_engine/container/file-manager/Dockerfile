FROM node:22-alpine
WORKDIR /mcp
COPY package.json package-lock.json ./
RUN npm ci --omit=dev \
    && npm cache clean --force \
    && rm -rf /root/.npm
ENV NODE_ENV=production
ENTRYPOINT ["node", "/mcp/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"]

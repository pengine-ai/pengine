FROM node:22-alpine AS builder
WORKDIR /app

ENV HUSKY=0
ENV NODE_ENV=production

COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts

COPY . .

ARG VITE_APP_ORIGIN=https://pengine.net
ENV VITE_APP_ORIGIN=${VITE_APP_ORIGIN}

RUN npm run build:web:ci

FROM node:22-alpine AS runner
WORKDIR /app

RUN apk add --no-cache curl

ENV NODE_ENV=production

COPY --from=builder /app /app

RUN chown -R node:node /app
USER node

EXPOSE 1422

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:1422/ >/dev/null || exit 1

CMD ["npm", "run", "preview", "--", "--host", "0.0.0.0", "--port", "1422"]

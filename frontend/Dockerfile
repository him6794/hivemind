# Official site: the static export from `next build`, served by scripts/serve-static-export.mjs.
# The account API is not in this image. Set API_ORIGIN and PROXY_SHARED_SECRET to forward /api/* to the
# website backend, as the Cloudflare Pages function does. Without both, /api/* answers 503.
FROM node:24-bookworm-slim AS build
ENV NEXT_TELEMETRY_DISABLED=1
WORKDIR /repo/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
ARG NEXT_PUBLIC_TURNSTILE_SITE_KEY
ENV NEXT_PUBLIC_TURNSTILE_SITE_KEY=${NEXT_PUBLIC_TURNSTILE_SITE_KEY}
# A stale export from the build context must never reach the image.
RUN rm -rf out && npm run build

FROM node:24-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production \
    HOST=0.0.0.0 \
    PORT=3000
COPY --from=build /repo/frontend/out ./out
COPY frontend/scripts/serve-static-export.mjs ./serve-static-export.mjs
USER node
EXPOSE 3000
CMD ["node", "serve-static-export.mjs", "--directory", "out"]

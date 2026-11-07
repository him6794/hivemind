FROM node:20-alpine

# Website backend for the account API. Built locally for linux/arm64 and loaded on the
# Orange Pi. This image only runs server.mjs. It holds no secrets: the proxy secret, the
# Turnstile secret (TURNSTILE_SECRET_KEY) and the Nodepool address come from the
# environment at run time.
#
# Build from the repository root. The named contexts supply the shared proto file and
# the shared helper modules, so the build context stays this small directory:
#   docker buildx build --platform linux/arm64 -f frontend/bff/Dockerfile \
#     --build-context proto=proto --build-context srclib=frontend/src/lib \
#     -t hivemind-bff:arm64 --load frontend/bff
WORKDIR /app/bff
ENV NODE_ENV=production \
    HOST=0.0.0.0 \
    PORT=3100

COPY package.json package-lock.json ./
RUN npm ci --omit=dev && npm cache clean --force

COPY server.mjs account-api.mjs turnstile.mjs ./
COPY --from=srclib rate-limit.mjs /app/src/lib/rate-limit.mjs
COPY --from=srclib account-request-policy.mjs /app/src/lib/account-request-policy.mjs
COPY --from=srclib account-input-policy.mjs /app/src/lib/account-input-policy.mjs
COPY --from=proto hivemind.proto /app/proto/hivemind.proto

# The server only reads these files and makes gRPC calls, so the image does not need root.
USER node

EXPOSE 3100
CMD ["node", "server.mjs"]

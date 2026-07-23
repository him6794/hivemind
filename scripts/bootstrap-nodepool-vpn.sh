#!/usr/bin/env bash
set -Eeuo pipefail

ROOT_DIR="${ROOT_DIR:-$(pwd)}"
ENV_FILE="${ENV_FILE:-$ROOT_DIR/.env}"
COMPOSE_FILE="${COMPOSE_FILE:-$ROOT_DIR/docker-compose.server-arm64.yml}"
COMPOSE=(docker compose --env-file "$ENV_FILE" -f "$COMPOSE_FILE")
PLACEHOLDER="bootstrap-placeholder"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

set_env() {
  local key="$1"
  local value="$2"
  local escaped
  escaped=$(printf '%s' "$value" | sed 's/[&|\\]/\\&/g')
  if grep -qE "^${key}=" "$ENV_FILE"; then
    sed -i "s|^${key}=.*|${key}=${escaped}|" "$ENV_FILE"
  else
    printf '\n%s=%s\n' "$key" "$value" >> "$ENV_FILE"
  fi
}

[ -f "$ENV_FILE" ] || die "missing $ENV_FILE"
[ -f "$COMPOSE_FILE" ] || die "missing $COMPOSE_FILE"
[ -c /dev/net/tun ] || die "/dev/net/tun is missing"

grep -q '^HEADSCALE_LOGIN_SERVER=.' "$ENV_FILE" || \
  die "HEADSCALE_LOGIN_SERVER is missing from $ENV_FILE"

# Nodepool refuses to start without a non-empty Ed25519 private key PEM.
# Empty values still interpolate through Compose and only crash at runtime.
if ! grep -qE '^WORKER_EXECUTION_PRIVATE_KEY_PEM=.+' "$ENV_FILE"; then
  die "WORKER_EXECUTION_PRIVATE_KEY_PEM is missing or empty in $ENV_FILE"
fi

existing_key=""
if grep -q '^NODEPOOL_VPN_AUTHKEY=.' "$ENV_FILE"; then
  existing_key=$(sed -n 's/^NODEPOOL_VPN_AUTHKEY=//p' "$ENV_FILE" | head -n1)
  [ "$existing_key" = "$PLACEHOLDER" ] && existing_key=""
fi

# Compose interpolation requires the variable even for `exec headscale`.
compose_with_placeholder=(env "NODEPOOL_VPN_AUTHKEY=${existing_key:-$PLACEHOLDER}" "${COMPOSE[@]}")

echo "Starting Headscale..."
"${compose_with_placeholder[@]}" up -d headscale >/dev/null

echo "Waiting for Headscale CLI..."
for _ in $(seq 1 30); do
  if "${compose_with_placeholder[@]}" exec -T headscale \
      /ko-app/headscale --config /etc/headscale/config.yaml version >/dev/null 2>&1; then
    break
  fi
  sleep 2
done

"${compose_with_placeholder[@]}" exec -T headscale \
  /ko-app/headscale --config /etc/headscale/config.yaml version >/dev/null 2>&1 || \
  die "Headscale is not ready; inspect: ${COMPOSE[*]} logs headscale"

echo "Ensuring Headscale user 'nodepool' exists..."
if ! "${compose_with_placeholder[@]}" exec -T headscale \
    /ko-app/headscale --config /etc/headscale/config.yaml users create nodepool 2>&1; then
  echo "User may already exist; continuing."
fi

if [ -z "$existing_key" ]; then
  echo "Creating reusable nodepool pre-auth key..."
  raw_key=$("${compose_with_placeholder[@]}" exec -T headscale \
    /ko-app/headscale --config /etc/headscale/config.yaml --output json \
    preauthkeys create --user nodepool --reusable --expiration 8760h)
  nodepool_key=$(printf '%s' "$raw_key" | sed -nE 's/.*"key"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' | head -n1)
  [ -n "$nodepool_key" ] || die "could not parse pre-auth key output: $raw_key"
  set_env NODEPOOL_VPN_AUTHKEY "$nodepool_key"
  echo "Saved NODEPOOL_VPN_AUTHKEY to $ENV_FILE"
else
  echo "NODEPOOL_VPN_AUTHKEY already exists; keeping it."
fi

echo "Starting VPN sidecar, nodepool and website-api..."
docker compose --env-file "$ENV_FILE" -f "$COMPOSE_FILE" \
  up -d --force-recreate nodepool-vpn nodepool website-api

echo "Checking VPN sidecar..."
docker compose --env-file "$ENV_FILE" -f "$COMPOSE_FILE" \
  exec -T nodepool-vpn tailscale status

echo "Bootstrap complete."

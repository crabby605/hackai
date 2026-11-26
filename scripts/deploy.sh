#!/usr/bin/env bash
# Build the Linux release locally and deploy API + standalone web to production.
# Usage: ./scripts/deploy.sh [--api-only|--web-only] [host]

set -euo pipefail

API_ONLY=false
WEB_ONLY=false
if [ "${1:-}" = "--api-only" ]; then
  API_ONLY=true
  shift
elif [ "${1:-}" = "--web-only" ]; then
  WEB_ONLY=true
  shift
fi
HOST="${1:-crabby605@hackclub.app}"
TARGET="x86_64-unknown-linux-gnu"
API_DIR="/root/hackai/bin"
WEB_DIR="/opt/hackai-web"

# Some SSH gateways return 255 after a successful remote command. Require an
# explicit completion marker emitted only after every remote command succeeds;
# never treat a transport error alone as proof of deployment success.
run_remote() {
  local marker="HACKAI_DEPLOY_OK_$$"
  local output
  local status=0
  output=$(ssh -T "$HOST" "set -euo pipefail
$1
printf '\\n%s\\n' '$marker'") || status=$?
  if ! grep -qx "$marker" <<< "$output"; then
    printf '%s\n' "$output" >&2
    if [ "$status" -eq 0 ]; then status=1; fi
    return "$status"
  fi
  printf '%s\n' "$output" | sed "/^${marker}$/d"
}

if [ "$WEB_ONLY" = false ]; then
  echo "Building API for $TARGET..."
  cargo zigbuild --release --target "$TARGET" -p hackai-server \
    --bin hackai-server --bin hackai-migrate
fi

if [ "$API_ONLY" = false ]; then
  echo "Building standalone web..."
  (
    cd web
    INTERNAL_API_URL="http://127.0.0.1:3000" npm run build
  )
fi

DEPLOY_STAGE=$(mktemp -d)
trap 'rm -rf "$DEPLOY_STAGE"' EXIT
REMOTE_STAGE="/tmp/hackai-deploy-$$"
if [ "$WEB_ONLY" = false ]; then
  cp "target/$TARGET/release/hackai-server" "$DEPLOY_STAGE/"
  cp "target/$TARGET/release/hackai-migrate" "$DEPLOY_STAGE/"
fi
if [ "$API_ONLY" = false ]; then
  mkdir -p "$DEPLOY_STAGE/web/.next"
  cp -R web/.next/standalone/. "$DEPLOY_STAGE/web/"
  cp -R web/.next/static "$DEPLOY_STAGE/web/.next/"
  if [ -d web/public ]; then cp -R web/public "$DEPLOY_STAGE/web/"; fi
fi

if [ "$WEB_ONLY" = false ]; then
echo "Deploying API to $HOST:$API_DIR..."
# Stage outside the live binary directory, then atomically replace the server
# during restart. Writing over a running executable fails with ETXTBSY.
run_remote "mkdir -p '$REMOTE_STAGE'"
scp -O "$DEPLOY_STAGE/hackai-server" "$DEPLOY_STAGE/hackai-migrate" "$HOST:$REMOTE_STAGE/"
fi
if [ "$API_ONLY" = false ]; then
  echo "Deploying web to $HOST:$WEB_DIR..."
  COPYFILE_DISABLE=1 tar --no-xattrs -C "$DEPLOY_STAGE/web" -czf - . | \
    run_remote "tar -xzf - -C '$WEB_DIR'"
fi

if [ "$WEB_ONLY" = false ]; then
run_remote 'set -euo pipefail
cp /root/hackai/bin/hackai-server /root/hackai/bin/hackai-server.previous
mv '"$REMOTE_STAGE"'/hackai-server /root/hackai/bin/hackai-server
mv '"$REMOTE_STAGE"'/hackai-migrate /root/hackai/bin/hackai-migrate
systemctl restart hackai-server
sleep 3
if ! curl -fsS http://127.0.0.1:3000/healthz >/dev/null; then
  mv /root/hackai/bin/hackai-server.previous /root/hackai/bin/hackai-server
  systemctl restart hackai-server
  exit 1
fi
rm -f /root/hackai/bin/hackai-server.previous
rm -rf '"$REMOTE_STAGE"'
systemctl is-active --quiet hackai-server
'
fi
if [ "$API_ONLY" = false ]; then
  run_remote 'set -euo pipefail
systemctl restart hackai-web
systemctl is-active --quiet hackai-web
curl --retry 10 --retry-connrefused --retry-delay 1 -fsS http://127.0.0.1:3001/health >/dev/null
'
fi

echo "Deployment complete: https://hackai.crabby605.hackclub.app"

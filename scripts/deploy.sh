#!/usr/bin/env bash
# HackAI production deployment script
# Usage: ./scripts/deploy.sh [host] [path]

set -euo pipefail

HOST="${1:-crabby605@hackclub}"
DEPLOY_PATH="${2:-/opt/hackai}"
REPO="https://github.com/crabby605/hackai.git"
BRANCH="main"

echo "🚀 Deploying HackAI to $HOST:$DEPLOY_PATH"

# 1. Build release binaries locally
echo "📦 Building release binaries..."
cargo build --workspace --release

# 2. Build web production bundle
echo "🌐 Building Next.js production bundle..."
cd web
NEXT_PUBLIC_API_URL="https://hackai.crabby605.hackclub.app" npm run build
cd ..

# 3. Create deployment package
echo "📦 Creating deployment package..."
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# Server binary
cp target/release/hackai-server "$TMPDIR/"

# Web build output
cp -r web/.next "$TMPDIR/web"
cp -r web/public "$TMPDIR/web"
cp web/package.json web/package-lock.json "$TMPDIR/web/"

# Config files
cp docker-compose.yml "$TMPDIR/"
cp .env.example "$TMPDIR/.env" 2>/dev/null || true

# Systemd service files
mkdir -p "$TMPDIR/systemd"
cat > "$TMPDIR/systemd/hackai-server.service" <<'EOF'
[Unit]
Description=HackAI Server
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=hackai
WorkingDirectory=/opt/hackai
Environment=DATABASE_URL=postgres://hackai:hackai@localhost:5432/hackai
Environment=HC_AUTH_CLIENT_ID=${HC_AUTH_CLIENT_ID}
Environment=HC_AUTH_CLIENT_SECRET=${HC_AUTH_CLIENT_SECRET}
Environment=SESSION_SIGNING_KEY=${SESSION_SIGNING_KEY}
Environment=FRONTEND_URL=https://hackai.crabby605.hackclub.app
Environment=HACKAI_PUBLIC_URL=https://hackai.crabby605.hackclub.app
ExecStart=/opt/hackai/hackai-server
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

cat > "$TMPDIR/systemd/hackai-web.service" <<'EOF'
[Unit]
Description=HackAI Web (Next.js)
After=network.target hackai-server.service
Requires=hackai-server.service

[Service]
Type=simple
User=hackai
WorkingDirectory=/opt/hackai/web
Environment=NODE_ENV=production
Environment=NEXT_PUBLIC_API_URL=https://hackai.crabby605.hackclub.app
ExecStart=/usr/bin/node server.js
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

# 4. Sync to remote
echo "📤 Syncing to $HOST:$DEPLOY_PATH..."
rsync -avz --delete \
  --exclude '.git' \
  --exclude 'target/debug' \
  --exclude 'node_modules' \
  --exclude '.next/cache' \
  "$TMPDIR/" "$HOST:$DEPLOY_PATH/"

# 5. Run remote setup
echo "🔧 Running remote setup..."
ssh "$HOST" <<'REMOTE_EOF'
set -euo pipefail

cd /opt/hackai

# Install web dependencies
cd web && npm ci --production && cd ..

# Setup systemd services
sudo cp systemd/hackai-server.service /etc/systemd/system/
sudo cp systemd/hackai-web.service /etc/systemd/system/
sudo systemctl daemon-reload

# Enable and restart services
sudo systemctl enable hackai-server hackai-web
sudo systemctl restart hackai-server
sleep 3
sudo systemctl restart hackai-web

# Check status
sudo systemctl status hackai-server --no-pager
sudo systemctl status hackai-web --no-pager
REMOTE_EOF

echo "✅ Deployment complete!"
echo "   Server: https://hackai.crabby605.hackclub.app"
echo "   API:    https://hackai.crabby605.hackclub.app/api/v1/health"
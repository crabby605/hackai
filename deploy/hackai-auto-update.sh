#!/bin/sh
set -eu
export PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin

LOCK=/run/lock/hackai-auto-update.lock
exec 9>"$LOCK"
flock -n 9 || exit 0

REPO=/root/hackai-git
cd "$REPO"
git fetch --quiet origin main
old=$(git rev-parse HEAD)
new=$(git rev-parse origin/main)
[ "$old" = "$new" ] && exit 0
git reset --hard --quiet origin/main

cargo build --release -p hackai-server >/root/hackai-src/build.log 2>&1
install -m 0755 target/release/hackai-server /root/hackai/bin/hackai-server

cd web
npm ci --ignore-scripts >/root/hackai-src/web-install.log 2>&1
npm run build >/root/hackai-src/web-build.log 2>&1
systemctl stop hackai-web.service
rm -rf /opt/hackai-web/.next /opt/hackai-web/node_modules /opt/hackai-web/server.js
cp -a .next/standalone/. /opt/hackai-web/
install -d -o hackai -g hackai /opt/hackai-web/.next
cp -a .next/static /opt/hackai-web/.next/
chown -R hackai:hackai /opt/hackai-web
systemctl start hackai-web.service
systemctl restart hackai-server.service
printf '%s updated to %s\n' hackai "$new"

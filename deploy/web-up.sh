#!/usr/bin/env bash
# The backtest site at https://backtests.signoscapital.com behind Google sign-in (PL-D29):
#   internet --443/80--> Caddy (TLS, Let's Encrypt) --> oauth2-proxy 127.0.0.1:4180 (Google,
#   @signoscapital.com only) --> the nginx container on 127.0.0.1:8080 (deploy/site-loop.sh's pages).
# Idempotent. Needs /etc/absolute-pipeline/google_oauth_client.json (the OAuth client of an Internal
# consent screen, redirect https://backtests.signoscapital.com/oauth2/callback) and the A record
# backtests -> this machine resolving, or Let's Encrypt cannot issue the certificate.
#   bash deploy/web-up.sh
set -euo pipefail
HOST=backtests.signoscapital.com
P=/srv/absolute/absolute-pipeline; E=/etc/absolute-pipeline; S=/srv/absolute
O2P_VERSION=v7.15.5
O2P_SHA256=f63f94bf72c5f46ab002a0a275aa8b3cf19b4d828aed08a13978cb9a62c3a1fd

sudo test -f $E/google_oauth_client.json || { echo "missing $E/google_oauth_client.json"; exit 1; }
addr=$(dig +short "$HOST" | tail -1); here=$(curl -s -m 5 https://checkip.amazonaws.com)
[ "$addr" = "$here" ] || { echo "$HOST resolves to '${addr:-nothing}', not this machine ($here)"; exit 1; }

# oauth2-proxy at a pinned release, checked against its published sha256.
if [ "$(/usr/local/bin/oauth2-proxy --version 2>/dev/null | grep -o 'v[0-9.]*' | head -1)" != "$O2P_VERSION" ]; then
  t=$(mktemp -d); f=oauth2-proxy-$O2P_VERSION.linux-amd64
  curl -sSLo "$t/$f.tar.gz" "https://github.com/oauth2-proxy/oauth2-proxy/releases/download/$O2P_VERSION/$f.tar.gz"
  echo "$O2P_SHA256  $t/$f.tar.gz" | sha256sum -c --quiet
  tar -xzf "$t/$f.tar.gz" -C "$t" && sudo install -m 0755 "$t/$f/oauth2-proxy" /usr/local/bin/oauth2-proxy && rm -rf "$t"
fi

# Its config: the client from the JSON, a cookie secret made once and kept; root's, 0600.
sudo test -f $E/oauth2-proxy.cookie || openssl rand -base64 32 | tr -- '+/' '-_' | sudo install -m 0600 /dev/stdin $E/oauth2-proxy.cookie
sudo python3 -I - "$P/deploy/web/oauth2-proxy.cfg.tmpl" "$E" <<'EOF'
import json, os, sys
tmpl, e = sys.argv[1], sys.argv[2]
client = json.load(open(f"{e}/google_oauth_client.json"))
client = client.get("web") or client.get("installed") or client
cfg = (open(tmpl).read()
       .replace("@CLIENT_ID@", client["client_id"])
       .replace("@CLIENT_SECRET@", client["client_secret"])
       .replace("@COOKIE_SECRET@", open(f"{e}/oauth2-proxy.cookie").read().strip()))
fd = os.open(f"{e}/oauth2-proxy.cfg", os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
os.write(fd, cfg.encode()); os.close(fd)
EOF

# The static site moves off the public port: only the proxy reaches it.
if ! docker port seed-site 2>/dev/null | grep -q '127.0.0.1:8080'; then
  docker rm -f seed-site >/dev/null 2>&1 || true
  docker run -d --name seed-site --restart unless-stopped -p 127.0.0.1:8080:80 -v $S/site:/usr/share/nginx/html:ro nginx:1.27-alpine >/dev/null
fi

# Caddy from its signed apt repository.
if ! command -v caddy >/dev/null; then
  sudo apt-get install -y -q debian-keyring debian-archive-keyring apt-transport-https >/dev/null
  curl -sSLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key | sudo gpg --batch --yes --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -sSLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt | sudo tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null
  sudo apt-get update -q >/dev/null && sudo apt-get install -y -q caddy >/dev/null
fi
sudo install -d -o caddy -g caddy /var/log/caddy
sudo install -m 0644 $P/deploy/web/Caddyfile /etc/caddy/Caddyfile

sudo install -m 0644 $P/deploy/systemd/oauth2-proxy.service /etc/systemd/system/oauth2-proxy.service
sudo systemctl daemon-reload
sudo systemctl enable --now oauth2-proxy && sudo systemctl restart oauth2-proxy
sudo systemctl enable --now caddy && sudo systemctl reload caddy
echo "https://$HOST: caddy $(systemctl is-active caddy), oauth2-proxy $(systemctl is-active oauth2-proxy), site $(docker port seed-site | head -1)"

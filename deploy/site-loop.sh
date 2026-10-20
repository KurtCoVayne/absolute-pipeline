#!/usr/bin/env bash
# The seed site on the machine: nginx on port 80 (the security group admits
# the owner's IP only) serving /srv/absolute/site, rebuilt every 60 s from
# the direct run's files by agents-harness/direct/site.py.
#   bash deploy/site-loop.sh        # (re)start nginx on :80 and the rebuild loop
#   bash deploy/site-loop.sh loop   # the loop itself (started by the line above)
set -uo pipefail
S=/srv/absolute; P=$S/absolute-pipeline
build() { python3 -I $P/agents-harness/direct/site.py --seeds $P/docs/design/fixtures/seed-posts.tsv,$P/docs/design/fixtures/campaign-2-posts.tsv,$P/docs/design/fixtures/campaign-3-posts.tsv --out $S/direct/out --site $S/site --mirror $S/data/seed; }
if [ "${1:-}" = "loop" ]; then
  while true; do build; sleep 60; done
fi
mkdir -p $S/site $S/direct/out $S/logs
build
docker rm -f seed-site >/dev/null 2>&1 || true
docker run -d --name seed-site --restart unless-stopped -p 0.0.0.0:80:80 -v $S/site:/usr/share/nginx/html:ro nginx:1.27-alpine >/dev/null
pkill -f "^bash $P/deploy/site-loop.sh loop" 2>/dev/null || true
nohup bash $P/deploy/site-loop.sh loop >> $S/logs/site.log 2>&1 &
echo "nginx on :80, rebuild loop started"

#!/usr/bin/env bash
# Requeue posts once memory allows: set their job dirs aside, mark them
# queued, and start a one-at-a-time driver when >= MIN_GB is available.
set -uo pipefail
POSTS="$1"; MIN_GB="${MIN_GB:-30}"; LOG=/srv/absolute/logs/direct-requeue-$(date +%H%M).log
cd /srv/absolute/direct
A=attic/requeue-$(date +%H%M); mkdir -p "$A"
for p in ${POSTS//,/ }; do
  [ -d "jobs/$p" ] && mv "jobs/$p" "$A/"
  python3 -c "
import json; f='out/$p/status.json'; s=json.load(open(f)); s['state']='queued'; s['stage']='quant'; json.dump(s,open(f,'w'))"
done
until [ "$(free -g | awk '/Mem:/{print $7}')" -ge "$MIN_GB" ]; do sleep 30; done
cd /srv/absolute/absolute-pipeline/agents-harness/direct
exec /srv/absolute/venv/bin/python run_direct.py --parallel 1 --only "$POSTS" > "$LOG" 2>&1

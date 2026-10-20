#!/usr/bin/env bash
# After the Norgate equities (stage-full.sh norgate): every current version of
# /srv/absolute/store-full marked reviewed, and the store measured.
set -uo pipefail
B=${B:-/srv/absolute/absolute-backtest/target/release/abt-data}
LOGS=/srv/absolute/logs/full
while pgrep -f "^bash /srv/absolute/full/stage-full.sh norgate" >/dev/null; do sleep 60; done
# A chunk that did not exit 0 runs again (the others are done before); its
# rows land inside the instruments' histories as late rows.
B=$B bash /srv/absolute/full/stage-full.sh norgate
python3 -I /srv/absolute/mark-reviewed.py /srv/absolute/store-full --all | tail -1
/usr/bin/time -v "$B" stats --store /srv/absolute/store-full --measure > "$LOGS/stats-measure.log" 2> "$LOGS/stats-measure.time"
echo "== store-full reviewed and measured $(date -u +%FT%TZ) $(du -sh /srv/absolute/store-full | cut -f1)" | tee -a "$LOGS/summary.log"

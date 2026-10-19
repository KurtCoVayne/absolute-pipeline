#!/usr/bin/env bash
# The futures build again with CL first (its first two ingests were stopped by
# a signal from outside), after the root in flight: stops the previous driver
# and chain by PID, waits for the ingest in flight, then CL, a serve snapshot,
# the remaining roots, the live/ extension, a snapshot, and the chain.
set -uo pipefail
export B=${B:-/srv/absolute/absolute-backtest/target/release/abt-data}
export STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures
S=/srv/absolute/full/stage-full.sh
self=$$
for p in $(pgrep -f "^bash /srv/absolute/full/(stage-futures.sh|chain-after-futures.sh|stage-full.sh dbn)"); do [ "$p" != "$self" ] && kill "$p"; done
while pgrep -f "^/srv/absolute/absolute-backtest/target/release/abt-data ingest --store /srv/absolute/store-futures" >/dev/null; do sleep 5; done
snapshot() {
  local tag=$1 dst=/srv/absolute/store-futures-serve
  rm -rf "$dst.new" && cp -a "$STORE" "$dst.new" && python3 -I /srv/absolute/mark-reviewed.py "$dst.new" --all | tail -1
  rm -rf "$dst.old"; [ -d "$dst" ] && mv "$dst" "$dst.old"; mv "$dst.new" "$dst"
  echo "== snapshot $tag $(date -u +%FT%TZ) $(du -sh $dst | cut -f1)" | tee -a "$LOGS/summary.log"
}
ONLY="CL" CAP=30G NEED_GB=32 bash $S dbn
snapshot cl-rb-ho-2
bash $S dbn
bash $S live
snapshot all
bash /srv/absolute/full/chain-after-futures.sh

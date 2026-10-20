#!/usr/bin/env bash
# The futures restage on E-109 (the ladder's source index built once per
# level): store-futures from empty, every GLBX root whole (no date splits:
# the continuous back-adjustment spans one ingest) from its outright-filtered
# day files, CL, RB and HO first and a serve snapshot of them; then the other
# roots, the live/ extension (SOL included, E-101), ICE IFUS.IMPACT, the
# TradeStation and Norgate continuous bundles, and a final reviewed snapshot,
# measured.
set -uo pipefail
export B=${B:-/srv/absolute/absolute-backtest/target/release/abt-data}
export STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures
S=/srv/absolute/full/stage-full.sh
snapshot() { # tag, destination
  local tag=$1 dst=$2
  rm -rf "$dst.new" && cp -a "$STORE" "$dst.new" && python3 -I /srv/absolute/mark-reviewed.py "$dst.new" --all | tail -1
  rm -rf "$dst.old"; [ -d "$dst" ] && mv "$dst" "$dst.old"; mv "$dst.new" "$dst"
  echo "== snapshot $tag $(date -u +%FT%TZ) $(du -sh "$dst" | cut -f1)" | tee -a "$LOGS/summary.log"
}
while pgrep -f "^bash /srv/absolute/full/outrights.sh" >/dev/null; do sleep 30; done
while pgrep -f "ts_to_bundle.py" >/dev/null; do sleep 30; done
[ -d "$LOGS" ] && mv "$LOGS" "$LOGS-$(date -u +%H%M)"
rm -rf "$STORE"; mkdir -p "$LOGS"
echo "== restage on $(sha256sum "$B" | cut -c1-12) $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
ONLY="CL RB HO" bash $S dbn
snapshot cl-rb-ho /srv/absolute/store-futures-serve
bash $S dbn
bash $S live
bash $S ifus
bash $S bundles
snapshot all /srv/absolute/store-futures-serve-all
"$B" stats --store /srv/absolute/store-futures-serve-all --measure > "$LOGS/stats-measure.log" 2>&1
echo "== futures done $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"

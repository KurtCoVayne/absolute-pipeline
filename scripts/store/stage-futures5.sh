#!/usr/bin/env bash
# The futures restage resumed (stage-futures4.sh paused at 04:00 for the
# second Norgate store's series and the Norgate 2015-2019 rerun): the store
# is kept, every root done before is skipped, the remaining GLBX roots, the
# live/ extension, ICE IFUS.IMPACT, the bundles, and the final reviewed
# snapshot, measured. A root's ingest holds the whole store's relation in
# memory (about 16-18 GB from the twentieth root on), so the remaining roots
# start from a measured 19 GB, not the estimate from their own rows.
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
while pgrep -f "^bash /srv/absolute/full/(nfut-series|finish-full|stage-futures4)\.sh" >/dev/null; do sleep 60; done
while pgrep -f "^bash /srv/absolute/full/stage-full\.sh" >/dev/null; do sleep 60; done
rm -f /srv/absolute/full/hold.txt
echo "== resumed on $(sha256sum "$B" | cut -c1-12) $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
bash $S dbn
bash $S live
bash $S ifus
bash $S bundles
snapshot all /srv/absolute/store-futures-serve-all
"$B" stats --store /srv/absolute/store-futures-serve-all --measure > "$LOGS/stats-measure.log" 2>&1
echo "== futures done $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"

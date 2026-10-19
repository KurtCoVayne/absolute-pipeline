#!/usr/bin/env bash
# The futures build from the priority file (first.txt: CL, RB, HO, then the
# other liquid roots), a serve snapshot after the first three, the remaining
# roots, the live/ extension, a final snapshot, and the chain.
set -uo pipefail
export B=${B:-/srv/absolute/absolute-backtest/target/release/abt-data}
export STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures
S=/srv/absolute/full/stage-full.sh
snapshot() {
  local tag=$1 dst=/srv/absolute/store-futures-serve
  rm -rf "$dst.new" && cp -a "$STORE" "$dst.new" && python3 -I /srv/absolute/mark-reviewed.py "$dst.new" --all | tail -1
  rm -rf "$dst.old"; [ -d "$dst" ] && mv "$dst" "$dst.old"; mv "$dst.new" "$dst"
  echo "== snapshot $tag $(date -u +%FT%TZ) $(du -sh $dst | cut -f1)" | tee -a "$LOGS/summary.log"
}
ONLY="CL RB HO" bash $S dbn
snapshot cl-rb-ho
bash $S dbn
bash $S live
snapshot all
bash /srv/absolute/full/chain-after-futures.sh

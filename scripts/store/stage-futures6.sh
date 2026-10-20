#!/usr/bin/env bash
# The futures restage resumed after the 04:30 OOM (futures from Databento
# only; the owner, Oct 20): the store is kept and every root done before is
# skipped; the remaining GLBX roots in first.txt's order, five at a time, a
# reviewed snapshot after each batch into a new directory
# (/srv/absolute/store-futures-serve-N, never the served one); then the live/
# extension, ICE IFUS.IMPACT and the TradeStation 1m bundles (their own roots,
# `<ROOT>.ts`), a snapshot after each. The reserve keeper withholds one slot.
set -uo pipefail
export B=${B:-/srv/absolute/absolute-backtest/target/release/abt-data}
export STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures
S=/srv/absolute/full/stage-full.sh
n=$(ls -d /srv/absolute/store-futures-serve-[0-9]* 2>/dev/null | sed 's/.*-//' | sort -n | tail -1); n=${n:-1}
snapshot() { # tag: a new numbered directory, reviewed; the third newest removed. A hard-link
  # copy of what is never rewritten (hot/segments, part/, log/: content-named or one file per
  # ingest) with its own registry/ and manifest/ (mark-reviewed rewrites them) and its own hot
  # version directories (their append files and journals grow in place at the next ingest).
  local tag=$1 dst
  n=$((n + 1)); dst=/srv/absolute/store-futures-serve-$n
  rm -rf "$dst.new"
  flock /srv/absolute/full/ingest.lock sh -c "cp -al '$STORE' '$dst.new' && rm -rf '$dst.new/registry' '$dst.new/manifest' && cp -a '$STORE/registry' '$STORE/manifest' '$dst.new/' \
    && for e in \$(ls '$STORE/hot'); do [ \$e = segments ] || { rm -rf '$dst.new/hot/'\$e && cp -a '$STORE/hot/'\$e '$dst.new/hot/'; } || exit 1; done" \
    || { echo "== snapshot $tag failed $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"; rm -rf "$dst.new"; n=$((n - 1)); return 1; }
  find "$dst.new" -maxdepth 3 -name '.staging-*' -exec rm -rf {} +
  python3 -I /srv/absolute/mark-reviewed.py "$dst.new" --all | tail -1
  mv "$dst.new" "$dst"
  echo "== snapshot $tag $dst $(date -u +%FT%TZ) $(du -sh "$dst" | cut -f1) roots: $(grep -oE '^(dbn|live|ifus|bundle)-[A-Za-z0-9-]+ exit 0' "$LOGS/summary.log" | cut -d' ' -f1 | tr '\n' ' ')" | tee -a "$LOGS/summary.log"
  rm -rf "/srv/absolute/store-futures-serve-$((n - 2))"
}
pgrep -f "^bash /srv/absolute/full/reserve-keeper.sh$" >/dev/null || setsid nohup bash /srv/absolute/full/reserve-keeper.sh > /srv/absolute/logs/reserve-keeper.log 2>&1 < /dev/null &
echo "== resumed on $(sha256sum "$B" | cut -c1-12) $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
left=$(for r in $(cat /srv/absolute/full/first.txt); do grep -q "^dbn-$r exit 0" "$LOGS/summary.log" || echo "$r"; done)
set -- $left
while [ $# -gt 0 ]; do
  batch="${1:-} ${2:-} ${3:-} ${4:-} ${5:-}"; shift $(( $# < 5 ? $# : 5 ))
  ONLY="$batch" bash $S dbn
  snapshot "dbn $(echo $batch)"
done
FIRST= bash $S dbn   # any GLBX root not in first.txt
bash $S live;    snapshot live
bash $S ifus;    snapshot ifus
BUNDLES='/srv/absolute/full/bundles/tradestation/*' bash $S bundles; snapshot bundles   # Databento and TradeStation only (no Norgate futures)
"$B" stats --store /srv/absolute/store-futures-serve-$n --measure > "$LOGS/stats-measure.log" 2>&1
echo "== futures done $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"

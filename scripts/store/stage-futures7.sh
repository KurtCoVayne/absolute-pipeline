#!/usr/bin/env bash
# The futures store from empty after E-121 (a killed ingest's next run rebuilt
# open/high/low from the base segments and dropped every other root's rows
# that lived only in the append files): Databento GLBX only, one root at a
# time in the posts' order, each ingest in a scope of its peak plus 8 GB; after
# each root the five bar relations consolidated (no root's rows left only in
# appends) and their row counts checked equal to the bars the roots kept (the
# driver stops if not); a hard-link snapshot after the first five roots and
# after every five more, into a new numbered directory, reviewed. 6C and ZQ
# (most bars without a definition) come last.
set -uo pipefail
export B=${B:-/srv/absolute/absolute-backtest/target/release/abt-data}
export STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures HEADROOM=8
S=/srv/absolute/full/stage-full.sh
PEAKS=/srv/absolute/full/peaks.txt
ORDER="ES NQ RTY BTC ETH CL RB HO GC SI ZN ZB ZF ZT 6E 6J NG ZC ZS ZW MES MNQ UB TN Z3N MCL QM NKD NIY QG
       6B HG PL MGC LE HE GF ZL ZM 6N 6L 6Z CNH 6M RP RY DC CSC CB DY GDK HRC LBS ALI TWE GE SR1 6C ZQ"
BARS="future.open future.high future.low future.close future.volume"
memwait() { while [ "$(free -g | awk '/Mem/{print $7}')" -lt "$1" ]; do sleep 30; done; }
peak_of() { local p; p=$(awk -v n="$1" '$1 == n {v = $2} END {print v}' "$PEAKS"); echo "${p:-$2}"; }
n=$(ls -d /srv/absolute/store-futures-serve-[0-9]* 2>/dev/null | sed 's/.*-//' | sort -n | tail -1); n=${n:-1}
snapshot() { # tag: a new numbered directory (see stage-futures6.sh), reviewed; the third newest removed
  local tag=$1 dst
  n=$((n + 1)); dst=/srv/absolute/store-futures-serve-$n
  rm -rf "$dst.new"
  flock /srv/absolute/full/ingest.lock sh -c "cp -al '$STORE' '$dst.new' && rm -rf '$dst.new/registry' '$dst.new/manifest' && cp -a '$STORE/registry' '$STORE/manifest' '$dst.new/' \
    && for e in \$(ls '$STORE/hot'); do [ \$e = segments ] || { rm -rf '$dst.new/hot/'\$e && cp -a '$STORE/hot/'\$e '$dst.new/hot/'; } || exit 1; done" \
    || { echo "== snapshot $tag failed $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"; rm -rf "$dst.new"; n=$((n - 1)); return 1; }
  find "$dst.new" -maxdepth 3 -name '.staging-*' -exec rm -rf {} +
  python3 -I /srv/absolute/mark-reviewed.py "$dst.new" --all | tail -1
  mv "$dst.new" "$dst"
  echo "== snapshot $tag $dst $(date -u +%FT%TZ) $(du -sh --apparent-size "$dst" | cut -f1) roots: $(grep -oE '^dbn-[A-Z0-9]+ exit 0' "$LOGS/summary.log" | cut -d' ' -f1 | sed 's/dbn-//' | tr '\n' ' ')" | tee -a "$LOGS/summary.log"
  rm -rf "/srv/absolute/store-futures-serve-$((n - 2))"
}
consolidate() { # root, relation: under the lock, in a scope of the last measured peak plus 8 GB
  local r=$1 rel=$2 peak code rss t0
  peak=$(peak_of "cons-$rel" 18)
  exec 9>>/srv/absolute/full/ingest.lock; flock 9
  memwait $(( peak + 2 ))
  t0=$(date +%s)
  sudo systemd-run --scope -p MemoryMax=$(( peak + 8 ))G -p MemorySwapMax=0 --uid=ubuntu --gid=ubuntu --quiet \
    /usr/bin/time -v eatmydata "$B" consolidate --store "$STORE" "$rel" > "$LOGS/cons-$r-$rel.log" 2> "$LOGS/cons-$r-$rel.time"
  code=$?
  flock -u 9
  rss=$(grep 'Maximum resident' "$LOGS/cons-$r-$rel.time" | awk '{print $NF}')
  echo "cons-$r-$rel exit $code wall $(( $(date +%s) - t0 ))s rss_kb $rss cap $(( peak + 8 ))G $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
  [ "$code" = 0 ] && [ -n "$rss" ] && echo "cons-$rel $(( (rss + 1048575) / 1048576 ))" >> "$PEAKS"
  return "$code"
}
stop() { echo "== STOPPED $* $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"; exit 1; }

[ -d "$LOGS" ] && mv "$LOGS" "$LOGS-pre-e121-$(date -u +%H%M)"
rm -rf "$STORE"; mkdir -p "$LOGS"
pgrep -f "^bash /srv/absolute/full/reserve-keeper.sh$" >/dev/null || setsid nohup bash /srv/absolute/full/reserve-keeper.sh > /srv/absolute/logs/reserve-keeper.log 2>&1 < /dev/null &
echo "== restage after E-121 on $(sha256sum "$B" | cut -c1-12) $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
kept=0 done_n=0 last=8
for r in $ORDER; do
  # The store's size drives an ingest's memory: never start below the last root's peak plus 1 GB.
  p=$(peak_of "dbn-$r" 0); [ "$p" -lt $(( last + 1 )) ] && echo "dbn-$r $(( last + 1 ))" >> "$PEAKS"
  ONLY="$r" bash $S dbn
  grep -q "^dbn-$r exit 0" "$LOGS/summary.log" || stop "dbn-$r did not go in"
  last=$(peak_of "dbn-$r" "$last")
  k=$(grep -oE '[0-9]+ bars kept' "$LOGS/dbn-$r.time" | cut -d' ' -f1); kept=$(( kept + ${k:-0} ))
  for rel in $BARS; do consolidate "$r" "$rel" || stop "consolidate $rel after $r"; done
  counts=$("$B" stats --store "$STORE" 2>/dev/null | awk '$1 ~ /^future\.(open|high|low|close|volume)$/ && /current/ {for (i = 1; i <= NF; i++) if ($i == "rows") print $1, $(i + 1)}')
  echo "rows after $r (kept $kept): $(echo $counts)" | tee -a "$LOGS/summary.log"
  [ "$(echo "$counts" | awk -v k="$kept" '$2 != k' | wc -l)" = 0 ] && [ "$(echo "$counts" | wc -l)" = 5 ] || stop "row counts after $r differ from $kept"
  done_n=$(( done_n + 1 ))
  [ $(( done_n % 5 )) = 0 ] && snapshot "$done_n roots"
done
[ $(( done_n % 5 )) = 0 ] || snapshot "$done_n roots"
for r in 6C ZQ; do echo "$r no_definition $(python3 -I -c "import json; d=json.load(open('$LOGS/dbn-$r-report.json')); print(d['dropped']['no_definition'], 'of', d['bars_in'], 'bars in;', d['bars_out'], 'out')")" | tee -a "$LOGS/summary.log"; done
echo "== glbx done $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"

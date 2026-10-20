#!/usr/bin/env bash
# The futures restage resumed after the 13:14 OOM (stage-futures7.sh stopped
# at GDK): the store is kept; a queue of `phase:ROOT` jobs (stage-full.sh's
# phases: dbn, extra, ifus, live), each ingest consolidated and its five bar
# relations checked equal to the bars kept so far (the driver stops if not);
# a hard-link snapshot after every five jobs into a new numbered directory.
# The rotation never removes the directory store-futures-current points to
# nor a store-futures-live-* (the coordinator's served copies), and each new
# snapshot is listed in snapshots.txt for the coordinator to repoint.
set -uo pipefail
export B=${B:-/srv/absolute/absolute-backtest/target/release/abt-data}
export STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures HEADROOM=8
S=/srv/absolute/full/stage-full.sh
PEAKS=/srv/absolute/full/peaks.txt
QUEUE=${QUEUE:-"dbn:GDK dbn:HRC dbn:LBS dbn:ALI dbn:TWE dbn:GE dbn:SR1
  extra:YM extra:ZO extra:ZR extra:PA extra:GD extra:6A extra:6S extra:DX extra:OJ
  ifus:CC ifus:CT ifus:KC ifus:SB ifus:RS ifus:MFS ifus:MME dbn:6C dbn:ZQ"}
BARS="future.open future.high future.low future.close future.volume"
memwait() { while [ "$(free -g | awk '/Mem/{print $7}')" -lt "$1" ]; do sleep 30; done; }
peak_of() { local p; p=$(awk -v n="$1" '$1 == n {v = $2} END {print v}' "$PEAKS"); echo "${p:-$2}"; }
counts() { "$B" stats --store "$STORE" 2>/dev/null | awk '$1 ~ /^future\.(open|high|low|close|volume)$/ && /current/ {for (i = 1; i <= NF; i++) if ($i == "rows") print $1, $(i + 1)}'; }
n=$(ls -d /srv/absolute/store-futures-serve-[0-9]* 2>/dev/null | sed 's/.*-//' | sort -n | tail -1); n=${n:-1}
snapshot() { # tag: a new numbered directory (see stage-futures6.sh), reviewed
  local tag=$1 dst old cur
  n=$((n + 1)); dst=/srv/absolute/store-futures-serve-$n
  rm -rf "$dst.new"
  flock /srv/absolute/full/ingest.lock sh -c "cp -al '$STORE' '$dst.new' && rm -rf '$dst.new/registry' '$dst.new/manifest' && cp -a '$STORE/registry' '$STORE/manifest' '$dst.new/' \
    && for e in \$(ls '$STORE/hot'); do [ \$e = segments ] || { rm -rf '$dst.new/hot/'\$e && cp -a '$STORE/hot/'\$e '$dst.new/hot/'; } || exit 1; done" \
    || { echo "== snapshot $tag failed $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"; rm -rf "$dst.new"; n=$((n - 1)); return 1; }
  find "$dst.new" -maxdepth 3 -name '.staging-*' -exec rm -rf {} +
  python3 -I /srv/absolute/mark-reviewed.py "$dst.new" --all | tail -1
  mv "$dst.new" "$dst"
  echo "== snapshot $tag $dst $(date -u +%FT%TZ) $(du -sh --apparent-size "$dst" | cut -f1) jobs: $(grep -oE '^[a-z]+-[A-Za-z0-9]+ exit 0' "$LOGS/summary.log" | cut -d' ' -f1 | tr '\n' ' ')" | tee -a "$LOGS/summary.log"
  echo "$dst $(date -u +%FT%TZ) $tag" >> "$LOGS/snapshots.txt"
  # Keep the newest two of this driver's snapshots and whatever is served.
  old=/srv/absolute/store-futures-serve-$((n - 2)); cur=$(readlink -f /srv/absolute/store-futures-current)
  [ -d "$old" ] && [ "$(readlink -f "$old")" != "$cur" ] && rm -rf "$old"
}
consolidate() { # job tag, relation: under the lock, in a scope of the last measured peak plus 8 GB
  local tag=$1 rel=$2 peak code rss t0
  peak=$(peak_of "cons-$rel" 18)
  exec 9>>/srv/absolute/full/ingest.lock; flock 9
  memwait $(( peak + 2 ))
  t0=$(date +%s)
  sudo systemd-run --scope -p MemoryMax=$(( peak + 8 ))G -p MemorySwapMax=0 --uid=ubuntu --gid=ubuntu --quiet \
    /usr/bin/time -v eatmydata "$B" consolidate --store "$STORE" "$rel" > "$LOGS/cons-$tag-$rel.log" 2> "$LOGS/cons-$tag-$rel.time"
  code=$?
  flock -u 9
  rss=$(grep 'Maximum resident' "$LOGS/cons-$tag-$rel.time" | awk '{print $NF}')
  echo "cons-$tag-$rel exit $code wall $(( $(date +%s) - t0 ))s rss_kb $rss cap $(( peak + 8 ))G $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
  [ "$code" = 0 ] && [ -n "$rss" ] && echo "cons-$rel $(( (rss + 1048575) / 1048576 ))" >> "$PEAKS"
  return "$code"
}
stop() { echo "== STOPPED $* $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"; exit 1; }

pgrep -f "^bash /srv/absolute/full/reserve-keeper.sh$" >/dev/null || setsid nohup bash /srv/absolute/full/reserve-keeper.sh > /srv/absolute/logs/reserve-keeper.log 2>&1 < /dev/null &
# The store as it stands: the bar relations must agree before anything is added.
kept=$(counts | awk 'NR == 1 {print $2}')
[ "$(counts | awk -v k="$kept" '$2 != k' | wc -l)" = 0 ] || stop "row counts differ before resuming: $(counts | tr '\n' ' ')"
echo "== resumed on $(sha256sum "$B" | cut -c1-12) $(date -u +%FT%TZ) with $kept rows" | tee -a "$LOGS/summary.log"
last=$(grep -E '^[a-z]+-[A-Za-z0-9]+ exit 0' "$LOGS/summary.log" | tail -1 | grep -oE 'rss_kb [0-9]+' | awk '{print int(($2 + 1048575) / 1048576)}'); last=${last:-30}
done_n=0
for job in $QUEUE; do
  phase=${job%%:*} r=${job#*:} name=${job%%:*}-${job#*:}
  grep -q "^$name exit 0" "$LOGS/summary.log" && continue
  # The store's size drives an ingest's memory: never start below the last ingest's peak plus 1 GB.
  p=$(peak_of "$name" 0); [ "$p" -lt $(( last + 1 )) ] && echo "$name $(( last + 1 ))" >> "$PEAKS"
  ONLY="$r" bash $S "$phase"
  grep -q "^$name exit 0" "$LOGS/summary.log" || { echo "== $name did not go in (no files, or refused): skipped $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"; continue; }
  last=$(peak_of "$name" "$last")
  k=$(grep -oE '[0-9]+ bars kept' "$LOGS/$name.time" | cut -d' ' -f1); kept=$(( kept + ${k:-0} ))
  for rel in $BARS; do consolidate "$name" "$rel" || stop "consolidate $rel after $name"; done
  c=$(counts)
  echo "rows after $name (kept $kept): $(echo $c)" | tee -a "$LOGS/summary.log"
  [ "$(echo "$c" | awk -v k="$kept" '$2 != k' | wc -l)" = 0 ] && [ "$(echo "$c" | wc -l)" = 5 ] || stop "row counts after $name differ from $kept"
  done_n=$(( done_n + 1 ))
  [ $(( done_n % 5 )) = 0 ] && snapshot "$done_n jobs"
done
[ $(( done_n % 5 )) = 0 ] || snapshot "$done_n jobs"
for r in 6C ZQ; do [ -f "$LOGS/dbn-$r-report.json" ] && echo "$r no_definition $(python3 -I -c "import json; d=json.load(open('$LOGS/dbn-$r-report.json')); print(d['dropped']['no_definition'], 'of', d['bars_in'], 'bars in;', d['bars_out'], 'out')")" | tee -a "$LOGS/summary.log"; done
echo "== queue done $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"

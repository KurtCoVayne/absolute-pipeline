#!/usr/bin/env bash
# The futures restage's job runner (stage-futures8.sh's, with the queue in a
# file): the next job of /srv/absolute/full/queue.txt (`phase:ROOT`, the phases
# of stage-full.sh: dbn, extra, ifus, live, comb) not done or skipped before,
# read again after every job so the queue can change while the driver runs;
# each ingest consolidated and its five bar relations checked equal to the
# rows expected (the driver stops if not). The counts come from the registry
# (stats would load every relation). A comb job re-ingests a root's history
# with its live/ extension: its rows revise the root's stored bars, so the
# expected total moves by its bars kept less the root's earlier ingest's. A
# hard-link snapshot after every five jobs into a new numbered directory; the
# rotation keeps what store-futures-current points to and every
# store-futures-live-*.
set -uo pipefail
export B=${B:-/srv/absolute/absolute-backtest/target/release/abt-data}
export STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures HEADROOM=8
# E-122 (an ingest builds only the instruments it writes): every ingest and consolidation in a
# fixed 12 GB scope, started once 16 GB are available, never retried (the coordinators, 15:15).
export CAP_GB=${CAP_GB:-12} GATE_GB=${GATE_GB:-16} MAX_TRIES=${MAX_TRIES:-1}
MIN_DISK_GB=${MIN_DISK_GB:-12}
S=/srv/absolute/full/stage-full.sh
PEAKS=/srv/absolute/full/peaks.txt
QUEUE_FILE=/srv/absolute/full/queue.txt
BARS="future.open future.high future.low future.close future.volume"
memwait() { while [ "$(free -g | awk '/Mem/{print $7}')" -lt "$1" ]; do sleep 30; done; }
peak_of() { local p; p=$(awk -v n="$1" '$1 == n {v = $2} END {print v}' "$PEAKS"); echo "${p:-$2}"; }
counts() { python3 -I -c "
import json
r = json.load(open('$STORE/registry/registry.json'))
for rel in '$BARS'.split():
    for src, vid in r['current'].get(rel, {}).items():
        print(rel, r['versions'][vid].get('rows'))"; }
kept_of() { grep -oE '[0-9]+ bars kept' "$LOGS/$1.time" 2>/dev/null | tail -1 | cut -d' ' -f1; }
next_job() { local j; for j in $(grep -v '^#' "$QUEUE_FILE"); do
  grep -qE "^(${j%%:*}-${j#*:} exit 0|== ${j%%:*}-${j#*:} skipped)" "$LOGS/summary.log" || { echo "$j"; return; }; done; }
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
  echo "== snapshot $tag $dst $(date -u +%FT%TZ) $(du -sh --apparent-size "$dst" | cut -f1) rows $(counts | awk 'NR == 1 {print $2}')" | tee -a "$LOGS/summary.log"
  echo "$dst $(date -u +%FT%TZ) $tag" >> "$LOGS/snapshots.txt"
  old=/srv/absolute/store-futures-serve-$((n - 2)); cur=$(readlink -f /srv/absolute/store-futures-current)
  [ -d "$old" ] && [ "$(readlink -f "$old")" != "$cur" ] && rm -rf "$old"
}
consolidate() { # job name, relation: under the lock, in a scope of the last measured peak plus 8 GB
  local tag=$1 rel=$2 peak code rss t0 cap
  peak=$(peak_of "cons-$rel" 18)
  exec 9>>/srv/absolute/full/ingest.lock; flock 9
  memwait "${GATE_GB:-$(( peak + 2 ))}"
  cap=${CAP_GB:-$(( peak + 8 ))}
  t0=$(date +%s)
  sudo systemd-run --scope -p MemoryMax=${cap}G -p MemorySwapMax=0 --uid=ubuntu --gid=ubuntu --quiet \
    /usr/bin/time -v eatmydata "$B" consolidate --store "$STORE" "$rel" > "$LOGS/cons-$tag-$rel.log" 2> "$LOGS/cons-$tag-$rel.time"
  code=$?
  flock -u 9
  rss=$(grep 'Maximum resident' "$LOGS/cons-$tag-$rel.time" | awk '{print $NF}')
  echo "cons-$tag-$rel exit $code wall $(( $(date +%s) - t0 ))s rss_kb $rss cap ${cap}G $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
  [ "$code" = 0 ] && [ -n "$rss" ] && echo "cons-$rel $(( (rss + 1048575) / 1048576 ))" >> "$PEAKS"
  return "$code"
}
stop() { echo "== STOPPED $* $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"; exit 1; }

while pgrep -f "^bash /srv/absolute/full/stage-full.sh" >/dev/null; do sleep 20; done   # a driver's ingest still finishing
pgrep -f "^bash /srv/absolute/full/reserve-keeper.sh$" >/dev/null || setsid nohup bash /srv/absolute/full/reserve-keeper.sh > /srv/absolute/logs/reserve-keeper.log 2>&1 < /dev/null &
# The relations the last (killed) ingest did not reach hold the store's settled rows; the
# next job (that ingest again) adds the same bars kept to all five.
kept=$(counts | awk '{print $2}' | sort -n | head -1)
echo "== resumed on $(sha256sum "$B" | cut -c1-12) $(date -u +%FT%TZ) with $kept rows ($(counts | tr '\n' ' '))" | tee -a "$LOGS/summary.log"
last=$(grep -E '^[a-z]+-[A-Za-z0-9]+ exit 0' "$LOGS/summary.log" | tail -1 | grep -oE 'rss_kb [0-9]+' | awk '{print int(($2 + 1048575) / 1048576)}'); last=${last:-30}
done_n=0
while job=$(next_job) && [ -n "$job" ]; do
  phase=${job%%:*} r=${job#*:} name=${job%%:*}-${job#*:}
  [ "$(df -BG --output=avail /srv | tail -1 | tr -dc 0-9)" -ge "$MIN_DISK_GB" ] || stop "under $MIN_DISK_GB GB of disk before $name"
  p=$(peak_of "$name" 0); [ "$p" -lt $(( last + 1 )) ] && echo "$name $(( last + 1 ))" >> "$PEAKS"
  ONLY="$r" bash $S "$phase"
  grep "^$name exit" "$LOGS/summary.log" | tail -1 | grep -qE "exit (137|143) " && stop "$name killed at its ${CAP_GB:-} GB scope"
  grep -q "^$name exit 0" "$LOGS/summary.log" || { echo "== $name skipped: did not go in (no files, or refused) $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"; continue; }
  last=$(peak_of "$name" "$last")
  k=$(kept_of "$name"); kept=$(( kept + ${k:-0} ))
  [ "$phase" = comb ] && kept=$(( kept - $(kept_of "dbn-$r") ))
  for rel in $BARS; do consolidate "$name" "$rel" || stop "consolidate $rel after $name"; done
  c=$(counts)
  echo "rows after $name (expected $kept): $(echo $c)" | tee -a "$LOGS/summary.log"
  [ "$(echo "$c" | awk -v k="$kept" '$2 != k' | wc -l)" = 0 ] && [ "$(echo "$c" | wc -l)" = 5 ] || stop "row counts after $name differ from $kept"
  done_n=$(( done_n + 1 ))
  [ $(( done_n % 5 )) = 0 ] && snapshot "$done_n jobs"
done
[ $(( done_n % 5 )) = 0 ] || snapshot "$done_n jobs"
for r in 6C ZQ; do [ -f "$LOGS/dbn-$r-report.json" ] && echo "$r no_definition $(python3 -I -c "import json; d=json.load(open('$LOGS/dbn-$r-report.json')); print(d['dropped']['no_definition'], 'of', d['bars_in'], 'bars in;', d['bars_out'], 'out')")" | tee -a "$LOGS/summary.log"; done
echo "== queue done $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"

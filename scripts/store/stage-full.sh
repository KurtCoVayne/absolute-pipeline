#!/usr/bin/env bash
# The full store (docs/iterations/01-data.md): every US list of the Norgate
# lake with the ETFs and the index series over their whole history, every
# Databento root (GLBX.MDP3, the live/ extension and the live-only roots,
# ICE IFUS.IMPACT), the Norgate continuous futures and TradeStation, built
# into /srv/absolute/store-full beside the live store. Each step waits for
# its own measured peak plus 2 GB available and runs in a scope of its peak
# plus 4 GB (peaks.txt): the live data service
# and the quant runs come first. A step done before is skipped (its line in
# the summary says exit 0). Usage: stage-full.sh norgate|dbn|live|ifus|bundles|all
set -uo pipefail
B=${B:-/srv/absolute/abt-full-target/release/abt-data}
STORE=${STORE:-/srv/absolute/store-full}
LOGS=${LOGS:-/srv/absolute/logs/full}; mkdir -p "$LOGS" "$STORE"
SUM="$LOGS/summary.log"; touch "$SUM"
L=/srv/absolute/data/vendor/ndlake
# Sessions of every Databento ingest, CME and ICE alike (E-108): the daily break's middle at 17:45
# New York and a 20-minute gap, so a bar in the break's first minute and the ES family's pre-2012
# 30-minute break both end a trade date.
DBN_SESSIONS="--session-break 17:45 --session-gap 20"
memwait() { while [ "$(free -g | awk '/Mem/{print $7}')" -lt "$1" ]; do sleep 30; done; }
PEAKS=/srv/absolute/full/peaks.txt
peak_gb() { # a step's measured peak in GB (peaks.txt, its latest line), else an estimate: a
  # prefiltered root's kept rows at 0.45 GB a million plus 2.5, else PEAK_GB (8)
  local name=$1 p done
  p=$(awk -v n="$name" '$1 == n {v = $2} END {print v}' "$PEAKS" 2>/dev/null)
  [ -n "$p" ] && { echo "$p"; return; }
  done="/srv/absolute/full/outrights/GLBX.MDP3/ohlcv-1m/${name#dbn-}.FUT.done"
  [[ $name == dbn-* ]] && [ -f "$done" ] && { awk '{printf "%d\n", $2 / 1e6 * 0.45 + 2.5}' "$done"; return; }
  echo "${PEAK_GB:-8}"
}
done_before() { grep -q "^$1 exit 0" "$SUM"; }
held() { # a root on hold (hold.txt: the roots whose sessions wait for an adapter fix)
  local root=${1##*-}
  [ -f /srv/absolute/full/hold.txt ] && grep -qw -- "$root" /srv/absolute/full/hold.txt
}
step() { # name, args...
  local name=$1; shift
  done_before "$name" && { echo "$name done before"; return 0; }
  held "$name" && { echo "$name held (hold.txt)"; return 0; }
  # Each ingest waits for its own peak plus 2 GB available and runs in a scope
  # of its peak plus 4 GB, so it can never take the data service down; killed
  # at the scope, it runs again with 8 GB more (twice at most). One ingest of
  # these drivers at a time, whichever store: two waiting on memory would
  # both see it free and start together.
  local peak code t0 rss try
  peak=$(peak_gb "$name")
  for try in 1 2 3; do
    exec 9>>/srv/absolute/full/ingest.lock; flock 9
    memwait $(( peak + 2 ))
    t0=$(date +%s)
    sudo systemd-run --scope -p MemoryMax=$(( peak + ${HEADROOM:-4} ))G -p MemorySwapMax=0 --uid=ubuntu --gid=ubuntu --quiet \
      /usr/bin/time -v eatmydata "$B" ingest --store "$STORE" "$@" > "$LOGS/$name.log" 2> "$LOGS/$name.time"
    code=$?
    flock -u 9
    rss=$(grep 'Maximum resident' "$LOGS/$name.time" | awk '{print $NF}')
    echo "$name exit $code wall $(( $(date +%s) - t0 ))s rss_kb $rss cap $(( peak + ${HEADROOM:-4} ))G $(date -u +%FT%TZ)" | tee -a "$SUM"
    grep -v '^\s' "$LOGS/$name.time" | grep -i -E 'error|refus|killed' | head -3
    case $code in 137|143) peak=$(( peak + 8 ));; *) break;; esac
  done
  [ "$code" = 0 ] && [ -n "$rss" ] && echo "$name $(( (rss + 1048575) / 1048576 ))" >> "$PEAKS"
  return 0
}
LISTS='S&P 500=SPX;S&P MidCap 400=SP400;S&P SmallCap 600=SP600;S&P Composite 1500=SP1500;S&P 100=SP100;Russell 3000=R3000;Russell 1000=R1000;Russell 2000=R2000;Russell Mid Cap=RMID;Russell Micro Cap=RMICRO;Russell Top 200=RTOP200;Russell 1000 + 2000 + Micro Cap Superset=RALL;Nasdaq Composite=NASDAQ;Nasdaq 100=NDX;Nasdaq Biotechnology=NBI;NYSE Composite=NYSE;Dow Jones Industrial Average=DJIA;S&P 500 Dividend Aristocrats=SPDA'
SERIES='$SPX=SPX;$SPXTR=SPXTR;$NDX=NDX;$NDXTR=NDXTR;$RUT=RUT;$RUTTR=RUTTR;$VIX=VIX_D;$DJI=DJI;$DJITR=DJITR;$RUA=RUA;$RUATR=RUATR;$COMP=COMP;$XCMP=XCMP;$SML=SML;$MID=MID;$OEX=OEX'
norgate() {
  local tickers; tickers=$(cat /srv/absolute/full/etf-tickers.txt)
  for span in 1990-01-01:1994-12-31 1995-01-01:1999-12-31 2000-01-01:2004-12-31 2005-01-01:2009-12-31 2010-01-01:2014-12-31 2015-01-01:2019-12-31 2020-01-01:2026-12-31; do
    step "norgate-${span%%:*}" --adapter norgate --from "$L" --lists "$LISTS" --series "$SERIES" --tickers "$tickers" \
      --start "${span%%:*}" --end "${span##*:}" --attributes reviewed=complete --report "$LOGS/norgate-${span%%:*}-report.json" --measure
  done
}
dbn_views() { # dataset dir, views dir
  local V=$1 VIEWS=$2 r
  for r in $(du -sm "$V"/ohlcv-1m/*.FUT 2>/dev/null | sort -n | awk '{print $2}' | xargs -n1 basename | sed 's/\.FUT$//'); do
    [ -f "$V/definition/$r.parquet" ] || { echo "$r: no definition" | tee -a "$SUM"; continue; }
    mkdir -p "$VIEWS/$r/definition" "$VIEWS/$r/ohlcv-1m"
    ln -sfn "$V/definition/$r.parquet" "$VIEWS/$r/definition/$r.parquet"
    local bars="$V/ohlcv-1m/$r.FUT" filt="/srv/absolute/full/outrights/$(basename "$V")/ohlcv-1m/$r.FUT"
    # A root prefiltered to its outright contracts (prefilter_outrights.py) reads the filtered day files.
    [ -d "$filt" ] && [ -f "$filt.done" ] && bars="$filt"
    ln -sfn "$bars" "$VIEWS/$r/ohlcv-1m/$r.FUT"
    echo "$r"
  done
}
order() { # the roots of `dbn_views`, the ones named in FIRST (or the file first.txt) first
  local all; all=$(cat)
  [ -z "${FIRST:-}" ] && [ -f /srv/absolute/full/first.txt ] && FIRST=$(cat /srv/absolute/full/first.txt)
  if [ -n "${ONLY:-}" ]; then for r in $ONLY; do echo "$all" | grep -qx "$r" && echo "$r"; done; return; fi
  for r in ${FIRST:-}; do echo "$all" | grep -qx "$r" && echo "$r"; done
  for r in $all; do case " ${FIRST:-} " in *" $r "*) ;; *) echo "$r";; esac; done
}
dbn() {
  for r in $(dbn_views /srv/absolute/data/vendor/dbn/GLBX.MDP3 /srv/absolute/full/views-dbn | order); do
    step "dbn-$r" --adapter databento --from "/srv/absolute/full/views-dbn/$r" --class future --undefined drop $DBN_SESSIONS --report "$LOGS/dbn-$r-report.json"
  done
}
live() {
  for r in $(dbn_views /srv/absolute/data/vendor/dbn-live/GLBX.MDP3 /srv/absolute/full/views-live); do
    step "live-$r" --adapter databento --from "/srv/absolute/full/views-live/$r" --class future --undefined drop $DBN_SESSIONS --report "$LOGS/live-$r-report.json"
  done
}
ifus() {
  for r in $(dbn_views /srv/absolute/data/vendor/dbn-ifus/IFUS.IMPACT /srv/absolute/full/views-ifus); do
    step "ifus-$r" --adapter databento --from "/srv/absolute/full/views-ifus/$r" --class future --undefined drop $DBN_SESSIONS --report "$LOGS/ifus-$r-report.json"
  done
}
bundles() { # bundle_v2 directories under /srv/absolute/full/bundles/<source>/<name> (BUNDLES: a narrower glob)
  for d in ${BUNDLES:-/srv/absolute/full/bundles/*/*}; do
    [ -f "$d/manifest.json" ] || continue
    local src; src=$(basename "$(dirname "$d")")
    step "bundle-$src-$(basename "$d")" --adapter bundle_v2 --from "$d" --class future --source "$src" --attributes reviewed=complete
  done
}
case "${1:-all}" in
  norgate) norgate;; dbn) dbn;; live) live;; ifus) ifus;; bundles) bundles;;
  all) norgate; bundles; dbn; live; ifus;;
esac
sync
echo "== stage-full ${1:-all} done $(date -u +%FT%TZ)" | tee -a "$SUM"

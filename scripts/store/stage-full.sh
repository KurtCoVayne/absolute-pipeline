#!/usr/bin/env bash
# The full store (docs/iterations/01-data.md): every US list of the Norgate
# lake with the ETFs and the index series over their whole history, every
# Databento root (GLBX.MDP3, the live/ extension and the live-only roots,
# ICE IFUS.IMPACT), the Norgate continuous futures and TradeStation, built
# into /srv/absolute/store-full beside the live store. Each step waits for
# 20 GB available and runs in a 20 GB memory scope: the live data service
# and the quant runs come first. A step done before is skipped (its line in
# the summary says exit 0). Usage: stage-full.sh norgate|dbn|live|ifus|bundles|all
set -uo pipefail
B=${B:-/srv/absolute/abt-full-target/release/abt-data}
STORE=${STORE:-/srv/absolute/store-full}
LOGS=${LOGS:-/srv/absolute/logs/full}; mkdir -p "$LOGS" "$STORE"
SUM="$LOGS/summary.log"; touch "$SUM"
L=/srv/absolute/data/vendor/ndlake
CAP=${CAP:-20G}
memwait() { while [ "$(free -g | awk '/Mem/{print $7}')" -lt "${NEED_GB:-20}" ]; do sleep 30; done; }
done_before() { grep -q "^$1 exit 0" "$SUM"; }
step() { # name, args...
  local name=$1; shift
  done_before "$name" && { echo "$name done before"; return 0; }
  memwait
  local t0; t0=$(date +%s)
  sudo systemd-run --scope -p MemoryMax=$CAP -p MemorySwapMax=0 --uid=ubuntu --gid=ubuntu --quiet \
    /usr/bin/time -v eatmydata "$B" ingest --store "$STORE" "$@" > "$LOGS/$name.log" 2> "$LOGS/$name.time"
  local code=$?
  echo "$name exit $code wall $(( $(date +%s) - t0 ))s rss_kb $(grep 'Maximum resident' "$LOGS/$name.time" | awk '{print $NF}') $(date -u +%FT%TZ)" | tee -a "$SUM"
  grep -v '^\s' "$LOGS/$name.time" | grep -i -E 'error|refus|killed' | head -3
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
    ln -sfn "$V/ohlcv-1m/$r.FUT" "$VIEWS/$r/ohlcv-1m/$r.FUT"
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
    step "dbn-$r" --adapter databento --from "/srv/absolute/full/views-dbn/$r" --class future --undefined drop --report "$LOGS/dbn-$r-report.json"
  done
}
live() {
  for r in $(dbn_views /srv/absolute/data/vendor/dbn-live/GLBX.MDP3 /srv/absolute/full/views-live); do
    [ "$r" = SOL ] && [ -z "${SOL_FIXED:-}" ] && { echo "live-SOL deferred: the continuous series stops at its first roll (adapter fix pending)" | tee -a "$SUM"; continue; }
    step "live-$r" --adapter databento --from "/srv/absolute/full/views-live/$r" --class future --undefined drop --report "$LOGS/live-$r-report.json"
  done
}
ifus() {
  for r in $(dbn_views /srv/absolute/data/vendor/dbn-ifus/IFUS.IMPACT /srv/absolute/full/views-ifus); do
    step "ifus-$r" --adapter databento --from "/srv/absolute/full/views-ifus/$r" --class future --undefined drop --report "$LOGS/ifus-$r-report.json"
  done
}
bundles() { # bundle_v2 directories under /srv/absolute/full/bundles/<source>/<name>
  for d in /srv/absolute/full/bundles/*/*; do
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

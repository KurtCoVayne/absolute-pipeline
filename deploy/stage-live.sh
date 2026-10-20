#!/usr/bin/env bash
# The maintenance window of Oct 19 (01-mvp.md): SOL in full and the
# Aug 17 - Sep 30 extension of the roots the seed posts name, from the
# bucket's live/ prefix, into the store with the data service stopped,
# under eatmydata (PL-D10) and one sync; then every current future and
# equity version marked reviewed (PL-D16) and the data service restarted.
set -uo pipefail
B=/srv/absolute/absolute-backtest/target/release
V=/srv/absolute/data/vendor/dbn-live/GLBX.MDP3
VIEWS=/srv/absolute/data/vendor/dbn-live-roots
STORE=/srv/absolute/store; LOGS=/srv/absolute/logs/dbn-live; mkdir -p "$LOGS" "$VIEWS"
pkill -f "^/srv/absolute/absolute-backtest/target/release/abt-data serve"; sleep 5
for r in ${ROOTS:-SOL ES NQ BTC ETH RTY MES MNQ}; do
  [ -f "$V/definition/$r.parquet" ] || { echo "$r: no definition"; continue; }
  mkdir -p "$VIEWS/$r/definition" "$VIEWS/$r/ohlcv-1m"
  ln -sfn "$V/definition/$r.parquet" "$VIEWS/$r/definition/$r.parquet"
  ln -sfn "$V/ohlcv-1m/$r.FUT" "$VIEWS/$r/ohlcv-1m/$r.FUT"
  /usr/bin/time -v eatmydata "$B/abt-data" ingest --store "$STORE" --adapter databento --from "$VIEWS/$r" --class future --undefined drop --session-break 17:45 --session-gap 20 \
    --report "$LOGS/$r-report.json" > "$LOGS/$r-ingest.log" 2> "$LOGS/$r-time.log"
  echo "$r exit $? wall $(grep Elapsed "$LOGS/$r-time.log" | awk '{print $NF}') rss_kb $(grep 'Maximum resident' "$LOGS/$r-time.log" | awk '{print $NF}') $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
  grep -v "^\s" "$LOGS/$r-time.log" | grep -i -E "error|refus" | head -3
done
sync
python3 -I /srv/absolute/mark-reviewed.py "$STORE" --all | tail -2
nohup "$B/abt-data" serve --store "$STORE" --socket /srv/absolute/run/data.sock --profile dedicated > /srv/absolute/logs/abt-data.log 2>&1 &
for i in $(seq 1 240); do [ -S /srv/absolute/run/data.sock ] && grep -q serving /srv/absolute/logs/abt-data.log && break; sleep 1; done
tail -1 /srv/absolute/logs/abt-data.log; echo "== live staging done $(date -u +%FT%TZ)"

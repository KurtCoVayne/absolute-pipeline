#!/usr/bin/env bash
# The roots the seed campaign needs first, under eatmydata (PL-D10), until a
# deadline; then one sync. The rest of the lake is debt (01-mvp.md).
set -uo pipefail
B=/srv/absolute/absolute-backtest/target/release
V=/srv/absolute/data/vendor/dbn/GLBX.MDP3
STORE=/srv/absolute/store; LOGS=/srv/absolute/logs/dbn; VIEWS=/srv/absolute/data/vendor/dbn-roots
DEADLINE=$(( $(date +%s) + ${MINUTES:-25} * 60 ))
for r in ${ROOTS:-ES NQ GC CL ZN 6E SI 6J HG ZF}; do
  [ "$(date +%s)" -ge "$DEADLINE" ] && { echo "deadline: stopped before $r"; break; }
  grep -q "^$r exit 0" "$LOGS/summary.log" && { echo "$r done before"; continue; }
  mkdir -p "$VIEWS/$r/definition" "$VIEWS/$r/ohlcv-1m"
  ln -sfn "$V/definition/$r.parquet" "$VIEWS/$r/definition/$r.parquet"
  ln -sfn "$V/ohlcv-1m/$r.FUT" "$VIEWS/$r/ohlcv-1m/$r.FUT"
  timeout $(( DEADLINE - $(date +%s) )) /usr/bin/time -v eatmydata "$B/abt-data" ingest --store "$STORE" --adapter databento --from "$VIEWS/$r" \
    --class future --undefined drop --report "$LOGS/$r-report.json" --measure > "$LOGS/$r-ingest.log" 2> "$LOGS/$r-time.log"
  code=$?
  echo "$r exit $code wall $(grep Elapsed "$LOGS/$r-time.log" | awk '{print $NF}') rss_kb $(grep 'Maximum resident' "$LOGS/$r-time.log" | awk '{print $NF}') $(date -u +%FT%TZ) eatmydata" | tee -a "$LOGS/summary.log"
done
sync; echo "== priority done $(date -u +%FT%TZ)"

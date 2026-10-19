#!/usr/bin/env bash
# The Databento lake staged one root at a time (PL-D8: the adapter holds a
# whole `--from` tree in memory before staging, and the full lake of 59
# roots took 31.6 GB and was killed; per root the peak is one root's bars).
# Each root gets a view directory with its definition and its bars; the
# roots go in by ascending size so a failure is late and alone; a root's
# exit code, wall time and peak RSS are logged.
set -uo pipefail
B=${B:-/srv/absolute/absolute-backtest/target/release}
V=${V:-/srv/absolute/data/vendor/dbn/GLBX.MDP3}
STORE=${STORE:-/srv/absolute/store}
LOGS=${LOGS:-/srv/absolute/logs/dbn}
VIEWS=${VIEWS:-/srv/absolute/data/vendor/dbn-roots}
mkdir -p "$LOGS" "$VIEWS"
roots=$(du -sm "$V"/ohlcv-1m/*.FUT | sort -n | awk '{print $2}' | xargs -n1 basename | sed 's/\.FUT$//')
for r in $roots; do
  [ -f "$V/definition/$r.parquet" ] || { echo "$r: no definition, skipped"; continue; }
  if grep -q "^$r exit 0" "$LOGS/summary.log" 2>/dev/null; then echo "$r: done before"; continue; fi
  mkdir -p "$VIEWS/$r/definition" "$VIEWS/$r/ohlcv-1m"
  ln -sfn "$V/definition/$r.parquet" "$VIEWS/$r/definition/$r.parquet"
  ln -sfn "$V/ohlcv-1m/$r.FUT" "$VIEWS/$r/ohlcv-1m/$r.FUT"
  /usr/bin/time -v "$B/abt-data" ingest --store "$STORE" --adapter databento --from "$VIEWS/$r" --class future --undefined drop \
    --report "$LOGS/$r-report.json" --measure > "$LOGS/$r-ingest.log" 2> "$LOGS/$r-time.log"
  code=$?
  echo "$r exit $code wall $(grep Elapsed "$LOGS/$r-time.log" | awk '{print $NF}') rss_kb $(grep 'Maximum resident' "$LOGS/$r-time.log" | awk '{print $NF}') $(date -u +%FT%TZ)" | tee -a "$LOGS/summary.log"
done
"$B/abt-data" stats --store "$STORE" --measure > "$LOGS/../stats-measure.log" 2>&1
du -sh "$STORE"; echo "== dbn roots done $(date -u +%FT%TZ)"

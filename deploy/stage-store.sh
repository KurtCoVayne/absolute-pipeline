#!/usr/bin/env bash
# ST-7's staging and ST-7.4's sizing run on the machine: the full Norgate
# history and the full Databento history from the synced vendor objects into
# one store, in measure mode, each stage's report kept. Idempotent: a
# relation already staged is a no-op for the adapter's classify stage.
set -uo pipefail
B=${B:-/srv/absolute/absolute-backtest/target/release}
V=${V:-/srv/absolute/data/vendor}
STORE=${STORE:-/srv/absolute/store}
LOGS=${LOGS:-/srv/absolute/logs}
mkdir -p "$STORE" "$LOGS"
T=/usr/bin/time
echo "== norgate $(date -u +%FT%TZ)"
$T -v "$B/abt-data" ingest --store "$STORE" --adapter norgate --from "$V/ndlake" ${NORGATE_ARGS:-} \
  --attributes reviewed=lake --report "$LOGS/norgate-report.json" --measure > "$LOGS/norgate-ingest.log" 2> "$LOGS/norgate-time.log"
echo "norgate exit $? $(date -u +%FT%TZ)"; tail -3 "$LOGS/norgate-ingest.log"; grep -E "Maximum resident|Elapsed" "$LOGS/norgate-time.log"
while ! grep -q dbn-done /srv/absolute/sync-vendor.log 2>/dev/null; do sleep 20; done
echo "== databento $(date -u +%FT%TZ)"
$T -v "$B/abt-data" ingest --store "$STORE" --adapter databento --from "$V/dbn/GLBX.MDP3" --class future --undefined drop ${DBN_ARGS:-} \
  --report "$LOGS/dbn-report.json" --measure > "$LOGS/dbn-ingest.log" 2> "$LOGS/dbn-time.log"
echo "databento exit $? $(date -u +%FT%TZ)"; tail -3 "$LOGS/dbn-ingest.log"; grep -E "Maximum resident|Elapsed" "$LOGS/dbn-time.log"
"$B/abt-data" stats --store "$STORE" --measure > "$LOGS/stats-measure.log" 2>&1
du -sh "$STORE"; echo "== done $(date -u +%FT%TZ)"

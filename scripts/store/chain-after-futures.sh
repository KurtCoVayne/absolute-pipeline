#!/usr/bin/env bash
# After the futures-first build (stage-futures.sh): the other futures sources
# into the same futures store (the Norgate continuous and TradeStation bundles,
# ICE IFUS.IMPACT) and a final reviewed serve snapshot; then the Norgate
# equities resume into /srv/absolute/store-full, marked reviewed and measured.
set -uo pipefail
S=/srv/absolute/full/stage-full.sh
while pgrep -f "^bash /srv/absolute/full/stage-futures.sh" >/dev/null; do sleep 60; done
while pgrep -f "ts_to_bundle.py" >/dev/null; do sleep 30; done
STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures bash $S bundles
STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures bash $S ifus
dst=/srv/absolute/store-futures-serve-all
rm -rf "$dst" && cp -a /srv/absolute/store-futures "$dst" && python3 -I /srv/absolute/mark-reviewed.py "$dst" --all | tail -1
echo "== futures snapshot with every source: $dst $(date -u +%FT%TZ) $(du -sh $dst | cut -f1)" | tee -a /srv/absolute/logs/futures/summary.log
STORE=/srv/absolute/store-full LOGS=/srv/absolute/logs/full bash $S norgate
python3 -I /srv/absolute/mark-reviewed.py /srv/absolute/store-full --all | tail -1
/srv/absolute/abt-full-target/release/abt-data stats --store /srv/absolute/store-full --measure > /srv/absolute/logs/full/stats-measure.log 2>&1
/srv/absolute/abt-full-target/release/abt-data stats --store /srv/absolute/store-futures-serve-all --measure > /srv/absolute/logs/futures/stats-measure.log 2>&1
echo "== chain done $(date -u +%FT%TZ)" | tee -a /srv/absolute/logs/full/summary.log

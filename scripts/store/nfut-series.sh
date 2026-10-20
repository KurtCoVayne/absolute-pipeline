#!/usr/bin/env bash
# The lake's index series into the second Norgate store (nfut-u.sh's last
# step), marked reviewed and verified: every series' span, and a daily ES_N
# program gated on VIX_D (probe_vix.dsl) run over 1997-2026 on the store. The
# adapter refuses a read without one equity bar, so SPY rides along as an ETF;
# the lake read from 1900 peaks near 15 GB.
set -uo pipefail
B=/srv/absolute/absolute-backtest/target/release
S=/srv/absolute/store-norgate-futures2
LOG=/srv/absolute/logs/nfut-u
SERIES=$(sed -n "s/^SERIES='\(.*\)'$/\1/p" /srv/absolute/full/nfut-u.sh)
exec 9>>/srv/absolute/full/ingest.lock; flock 9
while [ "$(free -g | awk '/Mem/{print $7}')" -lt 19 ]; do sleep 20; done
sudo systemd-run --scope -p MemoryMax=21G -p MemorySwapMax=0 --uid=ubuntu --gid=ubuntu --quiet /usr/bin/time -v eatmydata $B/abt-data ingest --store $S --adapter norgate --from /srv/absolute/data/vendor/ndlake --lists "" --tickers "SPY=ETF" --series "$SERIES" --start 1900-01-01 --attributes reviewed=complete --report $LOG/series-report.json > $LOG/series.log 2> $LOG/series.time
code=$?
echo "series exit $code $(grep Elapsed $LOG/series.time | awk '{print $NF}') $(grep Maximum $LOG/series.time | awk '{print $NF}') kB $(date -u +%FT%TZ)"
flock -u 9
exec 9>&-
[ "$code" = 0 ] || { grep -v '^\s' $LOG/series.time | tail -2; exit 1; }
sync
python3 -I /srv/absolute/mark-reviewed.py $S --all | tail -1
for n in $(echo "$SERIES" | tr ';' '\n' | cut -d= -f2); do
  q=$($B/abt-data query --store $S "series.value over 1900-01-01..2027-01-01 for $n" 2>/dev/null | grep -E '^\S+ \|')
  echo "$n rows $(echo "$q" | grep -c .) first $(echo "$q" | head -1 | cut -d'|' -f2 | cut -c2-11) last $(echo "$q" | tail -1 | cut -d'|' -f2 | cut -c2-11)"
done
L="/srv/absolute/absolute-backtest/corpus-v2/catalog/*.dsl /srv/absolute/absolute-backtest/corpus-v2/lib/*.dsl"
cd /srv/absolute/full && sudo systemd-run --scope -p MemoryMax=10G -p MemorySwapMax=0 --uid=ubuntu --gid=ubuntu --quiet \
  /usr/bin/time -f "probe run %e s %M kB" $B/abt run --strategy probe_es_vix_moo --data $S --start 1997-09-09 --end 2026-10-15 --timing $L probe_vix.dsl 2>&1 | tail -8
echo "== norgate futures store with _U and series ready $(date -u +%FT%TZ) $(du -sh $S | cut -f1)"

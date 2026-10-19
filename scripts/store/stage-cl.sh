#!/usr/bin/env bash
# CL alone, now: stops the futures driver between roots (by PID), stages CL
# from its outright-only day files under a 30 GB scope, snapshots the store
# for serving, and relaunches the driver (which skips the roots done).
set -uo pipefail
me=$$; parent=$PPID
F=/srv/absolute/full/filtered/GLBX.MDP3/ohlcv-1m
# Markers for the roots whose prefilter finished (its log lines; CL's ran first).
echo "87394989 29633399" > $F/CL.FUT.done
for r in $(sed -n 's/^\([A-Z0-9]*\): .*kept.*/\1/p' /srv/absolute/logs/prefilter.log); do [ -f $F/$r.FUT.done ] || echo done > $F/$r.FUT.done; done
for p in $(ps -eo pid,args | awk '/bash \/srv\/absolute\/full\/(stage-futures|stage-futures2|stage-futures3|chain-after-futures|stage-full|repoint)\.sh/ {print $1}'); do [ "$p" != "$me" ] && [ "$p" != "$parent" ] && kill "$p" 2>/dev/null; done
sleep 2
for p in $(ps -eo pid,args | awk '$2 ~ /abt-data$/ && /ingest --store \/srv\/absolute\/store-futures/ {print $1}'); do kill "$p" 2>/dev/null; done
for i in $(seq 1 30); do ps -eo args | grep -q "[i]ngest --store /srv/absolute/store-futures" || break; sleep 2; done
export B=/srv/absolute/absolute-backtest/target/release/abt-data STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures
ONLY=CL CAP=30G NEED_GB=32 bash /srv/absolute/full/stage-full.sh dbn
dst=/srv/absolute/store-futures-serve
rm -rf "$dst.new" && cp -a "$STORE" "$dst.new" && python3 -I /srv/absolute/mark-reviewed.py "$dst.new" --all | tail -1
rm -rf "$dst.old"; [ -d "$dst" ] && mv "$dst" "$dst.old"; mv "$dst.new" "$dst"
echo "== snapshot with-cl $(date -u +%FT%TZ) $(du -sh $dst | cut -f1)" | tee -a "$LOGS/summary.log"
nohup bash /srv/absolute/full/stage-futures3.sh > /srv/absolute/logs/futures/phase4.log 2>&1 < /dev/null &

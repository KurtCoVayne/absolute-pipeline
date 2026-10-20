#!/usr/bin/env bash
# The second Norgate store (store-norgate-futures2), built fresh beside the
# served one (whose log/ holds the service's derived relations): the
# back-adjusted continuous futures (<ROOT>_N, `&<ROOT>_CCB`), the unadjusted
# ones (<ROOT>_U, `&<ROOT>`) and the lake's index series as daily `series`
# (`$VIX` as VIX_D, the E-103 names, the Cboe volatility indexes), each from
# inception, marked reviewed. The served store is not written.
set -uo pipefail
B=/srv/absolute/absolute-backtest/target/release
S=/srv/absolute/store-norgate-futures2
LOG=/srv/absolute/logs/nfut-u; mkdir -p $LOG
SERIES='$VIX=VIX_D;$SPX=SPX;$SPXTR=SPXTR;$NDX=NDX;$NDXTR=NDXTR;$RUT=RUT;$RUTTR=RUTTR;$DJI=DJI;$DJITR=DJITR;$RUA=RUA;$RUATR=RUATR;$COMP=COMP;$XCMP=XCMP;$SML=SML;$MID=MID;$OEX=OEX;$VXN=VXN;$VVIX=VVIX;$VIX9D=VIX9D;$VIX3M=VIX3M;$VIX6M=VIX6M;$VIX1Y=VIX1Y;$VIX1D=VIX1D;$RVX=RVX;$OVX=OVX;$GVZ=GVZ;$VXEEM=VXEEM;$VXEFA=VXEFA;$VXEWZ=VXEWZ;$VXTLT=VXTLT'
if [ ! -f /srv/absolute/full/bundles/norgate-u/futures/manifest.json ]; then
  rm -rf /srv/absolute/full/bundles/norgate-u && mkdir -p /srv/absolute/full/bundles/norgate-u
  cd /tmp && nice -n 5 /srv/absolute/venv-data/bin/python -I /srv/absolute/full/scripts/norgate_futures_to_bundle.py /srv/absolute/data/vendor/ndlake /srv/absolute/full/bundles/norgate-u u
fi
rm -rf $S
ingest() { # name, args...
  local name=$1; shift
  while [ "$(free -g | awk '/Mem/{print $7}')" -lt 6 ]; do sleep 20; done
  sudo systemd-run --scope -p MemoryMax=8G -p MemorySwapMax=0 --uid=ubuntu --gid=ubuntu --quiet /usr/bin/time -v eatmydata $B/abt-data ingest --store $S "$@" > $LOG/$name.log 2> $LOG/$name.time
  echo "$name exit $? $(grep Elapsed $LOG/$name.time | awk '{print $NF}') $(grep Maximum $LOG/$name.time | awk '{print $NF}') kB $(date -u +%FT%TZ)"
}
exec 9>>/srv/absolute/full/ingest.lock; flock 9
ingest n --adapter bundle_v2 --from /srv/absolute/full/bundles/norgate/futures --class future --base 1d --attributes reviewed=complete
ingest u --adapter bundle_v2 --from /srv/absolute/full/bundles/norgate-u/futures --class future --base 1d --attributes reviewed=complete
# No list (the adapter's default universe is the S&P 500) and the lake's first day (its default start is 2014).
ingest series --adapter norgate --from /srv/absolute/data/vendor/ndlake --lists "" --series "$SERIES" --start 1900-01-01 --attributes reviewed=complete --report $LOG/series-report.json
flock -u 9
sync
python3 -I /srv/absolute/mark-reviewed.py $S --all | tail -1
echo "== norgate futures store with _U ready $(date -u +%FT%TZ) $(du -sh $S | cut -f1)"

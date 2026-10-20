#!/usr/bin/env bash
# The futures-only store first (the owner's priority, Oct 19 evening): CL, RB
# and HO into /srv/absolute/store-futures, then a reviewed snapshot of it to
# serve (/srv/absolute/store-futures-serve: a served store takes kernels'
# writes, so ingests never run against it), then every other GLBX.MDP3 root
# and the live/ extension and live-only roots into the build store.
set -uo pipefail
export STORE=/srv/absolute/store-futures LOGS=/srv/absolute/logs/futures
S=/srv/absolute/full/stage-full.sh
snapshot() { # the build store copied, marked reviewed, for serving
  local tag=$1 dst=/srv/absolute/store-futures-serve
  rm -rf "$dst.new" && cp -a "$STORE" "$dst.new" && python3 -I /srv/absolute/mark-reviewed.py "$dst.new" --all | tail -1
  rm -rf "$dst.old"; [ -d "$dst" ] && mv "$dst" "$dst.old"; mv "$dst.new" "$dst"
  echo "== snapshot $tag $(date -u +%FT%TZ) $(du -sh $dst | cut -f1)" | tee -a "$LOGS/summary.log"
}
ONLY="CL RB HO" bash $S dbn
snapshot cl-rb-ho
bash $S dbn
bash $S live
snapshot all

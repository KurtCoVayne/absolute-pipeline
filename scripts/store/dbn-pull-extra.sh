#!/usr/bin/env bash
# The Databento roots Norgate had and our pull lacked (the owner, Oct 20, 2026): ohlcv-1m
# and definitions in the bucket's layout, through d20-research's cost-gated writer
# (d20research/data/ingest.py: a free cost preview before every request, refused above
# $0.01, so only subscription-covered pulls), on the machine; then uploaded to
# s3://dbn-futures-094828274452 (versioned) under new keys only, with a sha256 manifest.
#   bash scripts/store/dbn-pull-extra.sh pull|upload|verify
set -uo pipefail
W=/srv/absolute/dbn-pull; CACHE=/srv/absolute/data/vendor/dbn-extra; LOG=/srv/absolute/logs/dbn-pull
BUCKET=s3://dbn-futures-094828274452
GLBX="EMD YM MYM M2K MBT MET KE ZO ZR PA SR3 GD LBR 6A 6S"; IFUS="DX OJ"
END=2026-10-19
mkdir -p "$CACHE" "$LOG"
case "${1:-pull}" in
pull)
  [ -x $W/venv/bin/python ] || { python3 -m venv $W/venv && $W/venv/bin/pip install -q databento pandas pyarrow; }
  export DATABENTO_API_KEY=$(sudo cat /etc/absolute-pipeline/databento_api_key)
  for r in $GLBX; do echo "GLBX.MDP3 $r 2010-06-06"; done > $W/jobs.txt
  for r in $IFUS; do echo "IFUS.IMPACT $r 2018-12-23"; done >> $W/jobs.txt
  cp /srv/absolute/absolute-pipeline/scripts/store/dbn_pull_extra.py $W/ && PYTHONPATH=$W/src $W/venv/bin/python $W/dbn_pull_extra.py "$CACHE" "$END" 2>&1 | tee -a $LOG/pull.log
  ;;
upload)
  # New keys only: a root's prefix is uploaded only when the bucket holds nothing under it.
  cd "$CACHE"
  find . -type f -name '*.parquet' | sed 's|^\./||' | sort | while read -r k; do
    printf '%s,%s,%s\n' "$k" "$(stat -c %s "$k")" "$(sha256sum "$k" | cut -d' ' -f1)"
  done > $LOG/manifest-additions.csv
  for d in */ohlcv-1m/*.FUT; do
    if [ -n "$(aws s3 ls "$BUCKET/$d/" | head -1)" ]; then echo "exists, skipped: $d" | tee -a $LOG/upload.log; continue; fi
    aws s3 sync "$d" "$BUCKET/$d/" --only-show-errors >> $LOG/upload.log 2>&1 && echo "uploaded $d $(ls $d | wc -l) files"
  done
  for f in */definition/*.parquet; do
    if aws s3api head-object --bucket dbn-futures-094828274452 --key "$f" >/dev/null 2>&1; then echo "exists, skipped: $f" | tee -a $LOG/upload.log; continue; fi
    aws s3 cp "$f" "$BUCKET/$f" --only-show-errors >> $LOG/upload.log 2>&1 && echo "uploaded $f"
  done
  { echo "path,bytes,sha256"; cat $LOG/manifest-additions.csv; } > $LOG/manifest-additions-2026-10-20.csv
  aws s3 cp $LOG/manifest-additions-2026-10-20.csv "$BUCKET/manifest-additions-2026-10-20.csv" --only-show-errors
  echo "manifest: $(wc -l < $LOG/manifest-additions.csv) files"
  ;;
verify)
  # Every manifest row is in the bucket with its size (one listing per root), and 20 files re-downloaded and re-hashed.
  : > $LOG/bucket-listing.txt
  for d in $(cut -d, -f1 $LOG/manifest-additions.csv | awk -F/ '{print $1"/"$2"/"$3}' | sort -u); do
    aws s3 ls "$BUCKET/$d" --recursive | awk '{print $4","$3}' >> $LOG/bucket-listing.txt
  done
  sort -u $LOG/bucket-listing.txt > $LOG/bucket-listing.sorted
  missing=$(cut -d, -f1,2 $LOG/manifest-additions.csv | sort | comm -23 - $LOG/bucket-listing.sorted | wc -l)
  shuf -n 20 $LOG/manifest-additions.csv | while IFS=, read -r k b h; do
    [ "$(aws s3 cp "$BUCKET/$k" - | sha256sum | cut -d' ' -f1)" = "$h" ] && echo ok || echo "HASH differs: $k"
  done | sort | uniq -c
  echo "verify: $missing of $(wc -l < $LOG/manifest-additions.csv) files missing or of another size in the bucket"
  ;;
esac

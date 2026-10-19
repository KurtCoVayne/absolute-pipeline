#!/usr/bin/env bash
# The 50 seed posts (docs/design/fixtures/seed-posts.tsv) as JSON files for
# the `file` adapter, one per post, from the public fxtwitter mirror
# (PL-D7: there is no X bearer token in SECRETS/ today; the X API's
# `GET /2/tweets?ids=` is the path once it exists: `ingest x lookup`).
# Read-only; nothing is republished. Output: data/seed/<post_id>.json with
# the mirror's record plus the seed row (handle, created_utc, url).
set -euo pipefail
cd "$(dirname "$0")/../.."
UA="absolute-pipeline/0.1 (research, read-only; contact: ${CONTACT:-owner})"
OUT=data/seed; mkdir -p "$OUT"
n=0; ok=0
while IFS=$'\t' read -r created handle id url; do
  [ "$created" = "created_utc" ] && continue
  [ -z "$id" ] && continue
  n=$((n+1))
  f="$OUT/$id.json"
  if [ -s "$f" ]; then ok=$((ok+1)); continue; fi
  if curl -fsS -m 20 -A "$UA" "https://api.fxtwitter.com/$handle/status/$id" -o "$f.tmp" && python3 -I -c "
import json,sys; d=json.load(open('$f.tmp')); assert d.get('code')==200 and d['tweet']['id']=='$id', d.get('code')
d['seed']={'handle':'$handle','post_id':'$id','created_utc':'$created','url':'$url','fetched_by':'fxtwitter','fetched_at':'$(date -u +%Y-%m-%dT%H:%M:%SZ)'}
json.dump(d,open('$f','w'))"; then ok=$((ok+1)); else echo "failed: $handle/$id"; fi
  rm -f "$f.tmp"; sleep 0.5
done < docs/design/fixtures/seed-posts.tsv
echo "$ok of $n seed posts in $OUT"

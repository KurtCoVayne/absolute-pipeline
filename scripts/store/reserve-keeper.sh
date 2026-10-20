#!/usr/bin/env bash
# Holds abt-slot's reservation (/srv/absolute/locks/reserve: the slots new
# quant runs leave free) while a staging driver runs: one slot, whatever the
# ingest (the coordinator, 04:10: three starved the backtest agents; quant runs
# are the OOM killer's first choice at oom_score_adj 500). Removed when the
# drivers end, or the keeper does.
R=/srv/absolute/locks/reserve
trap 'rm -f "$R" "$R.new"' EXIT
trap 'exit 0' TERM INT
while pgrep -f "^bash /srv/absolute/full/(stage-futures[0-9]|finish-full|stage-full|nfut-series)\.sh" >/dev/null; do
  echo 1 > "$R.new" && mv "$R.new" "$R"
  sleep 10
done

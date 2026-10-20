#!/usr/bin/env bash
# Holds abt-slot's reservation (/srv/absolute/locks/reserve: the slots new
# quant runs leave free) while a staging driver runs: one slot, two while an
# ingest's scope is 34 GB or more (the coordinator, 04:10: three starved the
# backtest agents; 14:30: two while a 30 GB ingest runs). Removed when the
# drivers end, or the keeper does.
R=/srv/absolute/locks/reserve
trap 'rm -f "$R" "$R.new"' EXIT
trap 'exit 0' TERM INT
while pgrep -f "^bash /srv/absolute/full/(stage-futures[0-9]|finish-full|stage-full|nfut-series)\.sh" >/dev/null; do
  cap=$(pgrep -af "systemd-run --scope -p MemoryMax=" | grep "ingest --store /srv/absolute/store-" | sed -n 's/.*MemoryMax=\([0-9]*\)G.*/\1/p' | head -1)
  w=1; [ -n "$cap" ] && [ "$cap" -ge 34 ] && w=2
  echo "$w" > "$R.new" && mv "$R.new" "$R"
  sleep 10
done

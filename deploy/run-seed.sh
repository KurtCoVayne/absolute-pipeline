#!/usr/bin/env bash
# The MVP as the owner cut it on Oct 19 (docs/iterations/01-mvp.md): on the
# machine, the three backtester programs on Unix sockets (dedicated profile,
# four kernels), the gateway, the single-process pipeline over a file-backed
# queue with the harness as a subprocess, and one static page served on
# 127.0.0.1:8080 (reached through an SSH tunnel). Idempotent: services that
# answer are left running; the queue's finished jobs are not rerun.
#   bash deploy/run-seed.sh prep|services|run|page|all
set -uo pipefail
S=/srv/absolute; B=$S/absolute-backtest/target/release; P=$S/absolute-pipeline; RUN=$S/run; LOGS=$S/logs; Q=${QUEUE:-$S/queue}
mkdir -p "$RUN" "$LOGS" "$S/site"
source ~/.cargo/env
prep() {
  # The backtester's crates from the synced amended tree (no GitHub credentials on the machine).
  sed -i -E 's#^(abt-[a-z]+) = \{ git = "https://github.com/KurtCoVayne/absolute-backtest", rev = "[0-9a-f]+" \}#\1 = { path = "/srv/absolute/absolute-backtest/crates/\1" }#' "$P/Cargo.toml"
  (cd "$P" && cargo build --release -p abt-gateway -p pipeline-run) || exit 1
  [ -f /etc/absolute-pipeline/token_signing_key ] || "$P/target/release/abt-gateway" token keygen /etc/absolute-pipeline/token_signing_key
  chmod 600 /etc/absolute-pipeline/*
}
up() { # name, socket, command...
  local name=$1 sock=$2; shift 2
  if [ -S "$sock" ] && pgrep -f -- "--socket $sock" >/dev/null; then echo "$name up"; return; fi
  rm -f "$sock"; nohup "$@" > "$LOGS/$name.log" 2>&1 &
  for i in $(seq 1 120); do [ -S "$sock" ] && { echo "$name serving ($i s)"; return; }; sleep 1; done
  echo "$name did not start: $(tail -3 $LOGS/$name.log)"; exit 1
}
services() {
  up abt-data $RUN/data.sock "$B/abt-data" serve --store $S/store --socket $RUN/data.sock --profile dedicated
  for k in 1 2 3 4; do up abt-kernel-$k $RUN/kernel-$k.sock "$B/abt-kernel" serve --socket $RUN/kernel-$k.sock --data $RUN/data.sock; done
  up abt-query $RUN/query.sock "$B/abt-query" serve --socket $RUN/query.sock --data $RUN/data.sock --kernel $RUN/kernel-1.sock
  if ! curl -fsS -m 5 http://127.0.0.1:7777/metrics >/dev/null 2>&1; then
    nohup "$P/target/release/abt-gateway" serve --data $RUN/data.sock --kernels $RUN/kernel-1.sock,$RUN/kernel-2.sock,$RUN/kernel-3.sock,$RUN/kernel-4.sock \
      --query $RUN/query.sock --library $S/absolute-backtest/corpus-v2/catalog --library $S/absolute-backtest/corpus-v2/lib \
      --listen 127.0.0.1:7777 --token-public-key /etc/absolute-pipeline/token_signing_key.pub > "$LOGS/abt-gateway.log" 2>&1 &
    for i in $(seq 1 60); do curl -fsS -m 5 http://127.0.0.1:7777/metrics >/dev/null 2>&1 && break; sleep 1; done
  fi
  curl -sS -m 10 http://127.0.0.1:7777/metrics | head -5
  docker ps --format '{{.Names}}' | grep -qx seed-site || docker run -d --name seed-site --restart unless-stopped -p 127.0.0.1:8080:80 -v $S/site:/usr/share/nginx/html:ro nginx:1.27-alpine >/dev/null
  echo "site on 127.0.0.1:8080"
}
run() {
  nohup "$P/target/release/pipeline-run" run --queue "$Q" --skills "$P/agents-skills" --python $S/venv/bin/python --harness-dir "$P/agents-harness" \
    --gateway http://127.0.0.1:7777 --gateway-bin "$P/target/release/abt-gateway" --signing-key /etc/absolute-pipeline/token_signing_key \
    --openai-key /etc/absolute-pipeline/openai_api_key --parallel ${PARALLEL:-14} --cap-usd ${CAP_USD:-50} --campaign seed-1 --gate ${GATE:-none} ${ONLY:+--only $ONLY} > "$LOGS/pipeline-run.log" 2>&1 &
  echo "pipeline-run started"
  ( while pgrep -f "pipeline-run run" >/dev/null; do "$P/target/release/pipeline-run" page --queue "$Q" --out $S/site/index.html 2>/dev/null; sleep 30; done
    "$P/target/release/pipeline-run" page --queue "$Q" --out $S/site/index.html ) > /dev/null 2>&1 &
}
page() { "$P/target/release/pipeline-run" page --queue "$Q" --out $S/site/index.html; }
case "${1:-all}" in prep) prep;; services) services;; run) run;; page) page;; all) prep; services; run;; esac

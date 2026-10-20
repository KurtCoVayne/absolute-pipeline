#!/usr/bin/env bash
# A Redpanda for the tests that need a broker (docs/design/contracts.md 1):
# `PIPELINE_BROKERS=127.0.0.1:19092`. Idempotent; `stop` removes it.
set -euo pipefail
NAME=absolute-redpanda-local
if [ "${1:-}" = "stop" ]; then docker rm -f "$NAME" >/dev/null 2>&1 || true; exit 0; fi
if ! docker ps --format '{{.Names}}' | grep -qx "$NAME"; then
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker run -d --name "$NAME" -p 127.0.0.1:19092:19092 -p 127.0.0.1:19644:9644 \
    docker.redpanda.com/redpandadata/redpanda:v25.2.1 redpanda start --mode dev-container --smp 1 --memory 1G \
    --kafka-addr internal://0.0.0.0:9092,external://0.0.0.0:19092 \
    --advertise-kafka-addr internal://127.0.0.1:9092,external://127.0.0.1:19092 >/dev/null
fi
for i in $(seq 1 30); do
  if docker exec "$NAME" rpk cluster health 2>/dev/null | grep -q "Healthy:.*true"; then echo "PIPELINE_BROKERS=127.0.0.1:19092"; exit 0; fi
  sleep 1
done
echo "redpanda did not become healthy" >&2; exit 1

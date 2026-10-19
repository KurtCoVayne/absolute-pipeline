#!/usr/bin/env bash
# On the machine: build both workspaces in release, build the images, create
# the sandbox network and the topics, and bring everything up with Compose.
# Idempotent. Run by scripts/aws/up.sh over SSH.
set -euo pipefail
cd /srv/absolute
source ~/.cargo/env
# The pipeline's git dependencies on absolute-backtest resolve to the synced tree (no GitHub credentials on the machine).
mkdir -p /srv/absolute/.cargo
cat > /srv/absolute/.cargo/config.toml <<'CFG'
[patch."https://github.com/KurtCoVayne/absolute-backtest"]
abt-wire = { path = "/srv/absolute/absolute-backtest/crates/abt-wire" }
abt-data = { path = "/srv/absolute/absolute-backtest/crates/abt-data" }
abt-kernel = { path = "/srv/absolute/absolute-backtest/crates/abt-kernel" }
abt-query = { path = "/srv/absolute/absolute-backtest/crates/abt-query" }
abt-tests = { path = "/srv/absolute/absolute-backtest/crates/abt-tests" }
abt-agg = { path = "/srv/absolute/absolute-backtest/crates/abt-agg" }
abt-codec = { path = "/srv/absolute/absolute-backtest/crates/abt-codec" }
CFG
(cd absolute-backtest && cargo build --release -p abt -p abt-data -p abt-kernel -p abt-query)
(cd absolute-pipeline && cargo build --release --workspace)
mkdir -p /srv/absolute/{run,store,data,jobs,site,redpanda,logs,corpus}
rsync -a --delete absolute-backtest/corpus-v2/ /srv/absolute/corpus/
sudo chown -R 1500:1500 /srv/absolute/run /srv/absolute/store 2>/dev/null || true
chmod 1777 /srv/absolute/run
bash absolute-pipeline/deploy/sandbox-network.sh
cd absolute-pipeline/deploy
docker compose -f compose.yaml build
docker build -t absolute/harness:$(git -C /srv/absolute/absolute-pipeline rev-parse --short HEAD 2>/dev/null || echo dev) -t absolute/harness:latest -f Dockerfile.harness ..
docker compose -f compose.yaml up -d redpanda
docker compose -f compose.yaml up -d abt-data abt-kernel-1 abt-kernel-2 abt-kernel-3 abt-kernel-4 abt-query
docker compose -f compose.yaml run --rm --no-deps ingest ingest topics create || true
docker compose -f compose.yaml up -d
docker compose -f compose.yaml ps

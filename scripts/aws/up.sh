#!/usr/bin/env bash
# Builds and starts everything on the machine from a clean tree: the three
# backtester programs, Redpanda, ingest, the orchestrator, the egress proxy,
# the gateway and the site, by Compose (deploy/compose.yaml). Idempotent:
# a second run rebuilds what changed and restarts what must.
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"
account; load_state
[ -n "${ADDRESS:-}" ] || { echo "no machine; run provision.sh" >&2; exit 1; }
ssh_machine 'cd /srv/absolute/absolute-pipeline && bash deploy/up.sh'

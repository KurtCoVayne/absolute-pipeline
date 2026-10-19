#!/usr/bin/env bash
# A shell on the machine: `scripts/aws/ssh.sh [command...]`.
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"
load_state
[ -n "${ADDRESS:-}" ] || { echo "no machine; run provision.sh" >&2; exit 1; }
ssh_machine "$@"

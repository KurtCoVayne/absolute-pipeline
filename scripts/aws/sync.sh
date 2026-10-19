#!/usr/bin/env bash
# Copies the two repositories and the secrets to the machine: the trees by
# rsync under /srv/absolute/ (no target/ directories, no SECRETS/), the
# secrets by scp into /etc/absolute-pipeline/ with mode 0600. Secrets never
# enter an image, a Compose file or the repositories.
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"
account; load_state
[ -n "${ADDRESS:-}" ] || { echo "no machine; run provision.sh" >&2; exit 1; }
RS=(rsync -az --delete -e "ssh -i $KEY_FILE -o StrictHostKeyChecking=accept-new" \
    --exclude target/ --exclude SECRETS/ --exclude SECRETS.received --exclude .venv/ --exclude __pycache__/ \
    --exclude 'data/' --exclude 'work/' --exclude '*.log')
"${RS[@]}" "$ROOT/" "ubuntu@$ADDRESS:/srv/absolute/absolute-pipeline/"
BACKTEST="${BACKTEST:-$ROOT/../absolute-backtest-amend}"
"${RS[@]}" --exclude 'experiments/' --exclude 'reference/' "$BACKTEST/" "ubuntu@$ADDRESS:/srv/absolute/absolute-backtest/"
for f in openai_api_key xai_api_key x_bearer_token token_signing_key; do
  if [ -f "$SECRETS/$f" ]; then
    scp -q -i "$KEY_FILE" "$SECRETS/$f" "ubuntu@$ADDRESS:/etc/absolute-pipeline/$f"
    ssh_machine "chmod 0600 /etc/absolute-pipeline/$f"
  fi
done
ssh_machine 'ls -l /etc/absolute-pipeline/; du -sh /srv/absolute/absolute-pipeline /srv/absolute/absolute-backtest'

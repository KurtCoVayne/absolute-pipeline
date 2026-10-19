#!/usr/bin/env bash
# Shared by provision.sh, sync.sh, up.sh and down.sh: the account, the region
# of the ndlake bucket, the machine's name and the files that hold its state.
set -euo pipefail
export AWS_PROFILE="${AWS_PROFILE:-signos}"
export AWS_DEFAULT_REGION="${AWS_DEFAULT_REGION:-us-east-1}"
export AWS_PAGER=""
NAME="${NAME:-absolute-pipeline}"
ACCOUNT_EXPECTED=094828274452
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SECRETS="$ROOT/SECRETS"
KEY_FILE="$SECRETS/machine_ed25519"
STATE="$HERE/machine.env"   # instance id and address; gitignored
BUCKETS=(ndlake-094828274452 dbn-futures-094828274452 orats-backup-094828274452)
INSTANCE_TYPE="${INSTANCE_TYPE:-m7i.2xlarge}"
VOLUME_GB="${VOLUME_GB:-300}"

account() {
  local acct
  acct=$(aws sts get-caller-identity --query Account --output text)
  if [ "$acct" != "$ACCOUNT_EXPECTED" ]; then
    echo "profile $AWS_PROFILE is account $acct, not the Signos account $ACCOUNT_EXPECTED" >&2
    exit 1
  fi
}

load_state() {
  if [ -f "$STATE" ]; then . "$STATE"; fi
  INSTANCE_ID="${INSTANCE_ID:-$(aws ec2 describe-instances \
    --filters "Name=tag:Name,Values=$NAME" "Name=instance-state-name,Values=pending,running,stopping,stopped" \
    --query 'Reservations[].Instances[].InstanceId' --output text | awk '{print $1}')}"
  if [ -n "${INSTANCE_ID:-}" ] && [ "$INSTANCE_ID" != "None" ]; then
    ADDRESS=$(aws ec2 describe-instances --instance-ids "$INSTANCE_ID" \
      --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)
  fi
}

save_state() {
  printf 'INSTANCE_ID=%s\nADDRESS=%s\n' "$INSTANCE_ID" "$ADDRESS" > "$STATE"
}

ssh_machine() {
  ssh -i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 \
      -o ServerAliveInterval=30 "ubuntu@$ADDRESS" "$@"
}

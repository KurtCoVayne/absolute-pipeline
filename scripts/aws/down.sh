#!/usr/bin/env bash
# Terminates the machine and deletes what provision.sh created around it
# (the security group, the role, the key pair). Not run on the day of the
# MVP (the report says the machine stays up); here so the machine can be
# recreated from nothing. Asks before terminating unless YES=1.
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"
account; load_state
if [ -n "${INSTANCE_ID:-}" ] && [ "$INSTANCE_ID" != "None" ]; then
  if [ "${YES:-0}" != "1" ]; then read -r -p "terminate $INSTANCE_ID ($ADDRESS)? [y/N] " a; [ "$a" = "y" ] || exit 1; fi
  aws ec2 terminate-instances --instance-ids "$INSTANCE_ID" >/dev/null
  aws ec2 wait instance-terminated --instance-ids "$INSTANCE_ID"
  echo "instance $INSTANCE_ID terminated"
fi
VPC=$(aws ec2 describe-vpcs --filters Name=isDefault,Values=true --query 'Vpcs[0].VpcId' --output text)
SG=$(aws ec2 describe-security-groups --filters "Name=group-name,Values=$NAME" "Name=vpc-id,Values=$VPC" --query 'SecurityGroups[0].GroupId' --output text 2>/dev/null || true)
[ -n "$SG" ] && [ "$SG" != "None" ] && aws ec2 delete-security-group --group-id "$SG" && echo "security group $SG deleted"
aws iam remove-role-from-instance-profile --instance-profile-name "$NAME-machine" --role-name "$NAME-machine" 2>/dev/null || true
aws iam delete-instance-profile --instance-profile-name "$NAME-machine" 2>/dev/null || true
aws iam delete-role-policy --role-name "$NAME-machine" --policy-name read-the-three-buckets 2>/dev/null || true
aws iam delete-role --role-name "$NAME-machine" 2>/dev/null || true
aws ec2 delete-key-pair --key-name "$NAME" 2>/dev/null || true
rm -f "$STATE"
echo "down"

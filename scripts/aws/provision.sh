#!/usr/bin/env bash
# Creates, from nothing and idempotently, the one machine of the plan (PL-3,
# PL-8.4): an m7i.2xlarge (8 vCPU, 32 GB) on Ubuntu 24.04 with 300 GB of gp3
# in the region of the ndlake bucket, an instance role that reads the three
# buckets, a security group with SSH from this machine's IP and nothing else
# inbound, and a cloud-init that installs Docker and Compose, gVisor as a
# Docker runtime, Rust stable, Python 3.12, and raises memlock for the store.
# Every resource is looked up by name before it is created, so the script can
# be rerun after a partial failure or after down.sh.
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"
account

MY_IP="${MY_IP:-$(curl -fsS https://checkip.amazonaws.com)}"
echo "account ok; region $AWS_DEFAULT_REGION; ssh from $MY_IP/32"

# The SSH key pair: created by ssh-keygen into SECRETS/ (never committed).
if [ ! -f "$KEY_FILE" ]; then
  ssh-keygen -t ed25519 -N "" -C "$NAME-machine" -f "$KEY_FILE" -q
fi
if ! aws ec2 describe-key-pairs --key-names "$NAME" >/dev/null 2>&1; then
  aws ec2 import-key-pair --key-name "$NAME" --public-key-material "fileb://$KEY_FILE.pub" >/dev/null
  echo "key pair $NAME imported"
fi

# The role that reads the three buckets.
if ! aws iam get-role --role-name "$NAME-machine" >/dev/null 2>&1; then
  aws iam create-role --role-name "$NAME-machine" --assume-role-policy-document '{
    "Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}' >/dev/null
  echo "role $NAME-machine created"
fi
RES=""; for b in "${BUCKETS[@]}"; do RES="$RES\"arn:aws:s3:::$b\",\"arn:aws:s3:::$b/*\","; done
aws iam put-role-policy --role-name "$NAME-machine" --policy-name read-the-three-buckets --policy-document "{
  \"Version\":\"2012-10-17\",\"Statement\":[
    {\"Effect\":\"Allow\",\"Action\":[\"s3:GetObject\",\"s3:ListBucket\",\"s3:GetBucketLocation\"],\"Resource\":[${RES%,}]}]}"
if ! aws iam get-instance-profile --instance-profile-name "$NAME-machine" >/dev/null 2>&1; then
  aws iam create-instance-profile --instance-profile-name "$NAME-machine" >/dev/null
  aws iam add-role-to-instance-profile --instance-profile-name "$NAME-machine" --role-name "$NAME-machine"
  echo "instance profile $NAME-machine created"; sleep 10
fi

# The security group: SSH from this IP only.
VPC=$(aws ec2 describe-vpcs --filters Name=isDefault,Values=true --query 'Vpcs[0].VpcId' --output text)
SG=$(aws ec2 describe-security-groups --filters "Name=group-name,Values=$NAME" "Name=vpc-id,Values=$VPC" \
     --query 'SecurityGroups[0].GroupId' --output text 2>/dev/null || true)
if [ -z "$SG" ] || [ "$SG" = "None" ]; then
  SG=$(aws ec2 create-security-group --group-name "$NAME" --description "$NAME: ssh from the owner machine only" \
       --vpc-id "$VPC" --query GroupId --output text)
  echo "security group $SG created"
fi
# Replace every ingress rule by the one wanted (the IP may have changed).
CURRENT=$(aws ec2 describe-security-groups --group-ids "$SG" --query 'SecurityGroups[0].IpPermissions' --output json)
if [ "$CURRENT" != "[]" ]; then
  aws ec2 revoke-security-group-ingress --group-id "$SG" --ip-permissions "$CURRENT" >/dev/null
fi
aws ec2 authorize-security-group-ingress --group-id "$SG" --ip-permissions \
  "[{\"IpProtocol\":\"tcp\",\"FromPort\":22,\"ToPort\":22,\"IpRanges\":[{\"CidrIp\":\"$MY_IP/32\",\"Description\":\"owner\"}]}]" >/dev/null
echo "security group $SG: tcp/22 from $MY_IP/32 only"

# The instance.
load_state
if [ -z "${INSTANCE_ID:-}" ] || [ "$INSTANCE_ID" = "None" ]; then
  AMI=$(aws ssm get-parameter --name /aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id \
        --query Parameter.Value --output text)
  SUBNET=$(aws ec2 describe-subnets --filters "Name=vpc-id,Values=$VPC" "Name=default-for-az,Values=true" \
           "Name=availability-zone,Values=${AZ:-us-east-1a}" --query 'Subnets[0].SubnetId' --output text)
  INSTANCE_ID=$(aws ec2 run-instances --image-id "$AMI" --instance-type "$INSTANCE_TYPE" --key-name "$NAME" \
    --security-group-ids "$SG" --subnet-id "$SUBNET" --associate-public-ip-address \
    --iam-instance-profile "Name=$NAME-machine" \
    --block-device-mappings "[{\"DeviceName\":\"/dev/sda1\",\"Ebs\":{\"VolumeSize\":$VOLUME_GB,\"VolumeType\":\"gp3\",\"DeleteOnTermination\":true}}]" \
    --metadata-options HttpTokens=required,HttpPutResponseHopLimit=2 \
    --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=$NAME}]" "ResourceType=volume,Tags=[{Key=Name,Value=$NAME}]" \
    --user-data "file://$HERE/cloud-init.yaml" \
    --query 'Instances[0].InstanceId' --output text)
  echo "instance $INSTANCE_ID launched ($INSTANCE_TYPE, $AMI, $VOLUME_GB GB gp3)"
else
  STATE_NOW=$(aws ec2 describe-instances --instance-ids "$INSTANCE_ID" --query 'Reservations[0].Instances[0].State.Name' --output text)
  if [ "$STATE_NOW" = "stopped" ]; then aws ec2 start-instances --instance-ids "$INSTANCE_ID" >/dev/null; fi
  echo "instance $INSTANCE_ID exists ($STATE_NOW)"
fi
aws ec2 wait instance-running --instance-ids "$INSTANCE_ID"
ADDRESS=$(aws ec2 describe-instances --instance-ids "$INSTANCE_ID" --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)
save_state
echo "instance $INSTANCE_ID at $ADDRESS; waiting for cloud-init"
for i in $(seq 1 60); do
  if ssh_machine 'cloud-init status --wait >/dev/null 2>&1; test -f /var/lib/cloud/instance/boot-finished' 2>/dev/null; then break; fi
  sleep 10
done
ssh_machine 'echo "$(nproc) vCPU, $(free -g | awk "/Mem/{print \$2}") GB, $(uname -r), $(lsb_release -ds)"; docker --version; docker compose version; runsc --version | head -1; ~/.cargo/bin/rustc --version; python3.12 --version; ulimit -l; df -h / | tail -1'
echo "machine ready: $INSTANCE_ID $ADDRESS"

#!/usr/bin/env bash
PATH=$PWD/bin:$PATH
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# lamports=100000000000000
dif=100000000000000
# bootstrap_leader_lamports=
bootstrap_leader_dif=

usage () {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [-n dif] [-b dif]

Create a cluster configuration

 -n dif    - Number of dif to create [default: $dif]
 -b dif    - Override the number of dif for the bootstrap leader's stake

EOF
  exit $exitcode
}

while getopts "h?n:b:" opt; do
  case $opt in
  h|\?)
    usage
    exit 0
    ;;
  n)
    # lamports="$OPTARG"
    dif="$OPTARG"
    ;;
  b)
    # bootstrap_leader_lamports="$OPTARG"
    bootstrap_leader_dif="$OPTARG"
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done


set -e
"$here"/clear-fullnode-config.sh

# Create genesis ledger
soros-keygen -o "$SOROS_CONFIG_DIR"/mint-id.json
soros-keygen -o "$SOROS_CONFIG_DIR"/bootstrap-leader-id.json
soros-keygen -o "$SOROS_CONFIG_DIR"/bootstrap-leader-vote-id.json

args=(
  --bootstrap-leader-keypair "$SOROS_CONFIG_DIR"/bootstrap-leader-id.json
  --bootstrap-vote-keypair "$SOROS_CONFIG_DIR"/bootstrap-leader-vote-id.json
  --ledger "$SOROS_RSYNC_CONFIG_DIR"/ledger
  --mint "$SOROS_CONFIG_DIR"/mint-id.json
  # --lamports "$lamports"
  --dif "$dif"
)

# if [[ -n $bootstrap_leader_lamports ]]; then
if [[ -n $bootstrap_leader_dif ]]; then
  # args+=(--bootstrap-leader-lamports "$bootstrap_leader_lamports")
  args+=(--bootstrap-leader-dif "$bootstrap_leader_dif")
fi

soros-genesis "${args[@]}"
cp -a "$SOROS_RSYNC_CONFIG_DIR"/ledger "$SOROS_CONFIG_DIR"/bootstrap-leader-ledger

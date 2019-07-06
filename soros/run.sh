#!/usr/bin/env bash
#
# Run a minimal Soros cluster.  Ctrl-C to exit.
#
# Before running this script ensure standard Soros programs are available
# in the PATH, or that `cargo build --all` ran successfully
#
set -e

# Prefer possible `cargo build --all` binaries over PATH binaries
PATH=$PWD/target/debug:$PATH

ok=true
for program in soros-{genesis,keygen,fullnode}; do
  $program -V || ok=false
done
$ok || {
  echo
  echo "Unable to locate required programs.  Try building them first with:"
  echo
  echo "  $ cargo build --all"
  echo
  exit 1
}

blockstreamSocket=/tmp/bitconch-blockstream.sock # Default to location used by the block explorer
while [[ -n $1 ]]; do
  if [[ $1 = --blockstream ]]; then
    blockstreamSocket=$2
    shift 2
  else
    echo "Unknown argument: $1"
    exit 1
  fi
done

export RUST_LOG=${RUST_LOG:-soros=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1
dataDir=$PWD/target/"$(basename "$0" .sh)"

set -x
soros-keygen -o "$dataDir"/config/leader-keypair.json
soros-keygen -o "$dataDir"/config/leader-vote-account-keypair.json
soros-keygen -o "$dataDir"/config/drone-keypair.json

leaderVoteAccountPubkey=$(\
  soros-wallet \
    --keypair "$dataDir"/config/leader-vote-account-keypair.json  \
    address \
)

soros-genesis \
  # --lamports 1000000000 \
  --dif 1000000000 \
  --mint "$dataDir"/config/drone-keypair.json \
  --bootstrap-leader-keypair "$dataDir"/config/leader-keypair.json \
  --bootstrap-vote-keypair "$dataDir"/config/leader-vote-account-keypair.json \
  --ledger "$dataDir"/ledger

soros-drone --keypair "$dataDir"/config/drone-keypair.json &
drone=$!

args=(
  --identity "$dataDir"/config/leader-keypair.json
  --voting-keypair "$dataDir"/config/leader-vote-account-keypair.json
  --vote-account "$leaderVoteAccountPubkey"
  --ledger "$dataDir"/ledger/
  # --rpc-port 8899
  --rpc-port 10099
  # --rpc-drone-address 127.0.0.1:9900
  --rpc-drone-address 127.0.0.1:11100
)
if [[ -n $blockstreamSocket ]]; then
  args+=(--blockstream "$blockstreamSocket")
fi
soros-fullnode "${args[@]}" &
fullnode=$!

abort() {
  set +e
  kill "$drone" "$fullnode"
}
trap abort INT TERM EXIT

wait "$fullnode"

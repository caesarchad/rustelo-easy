#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/..

cargo build
export PATH=$PWD/target/debug:$PATH

echo "\`\`\`manpage"
soros-wallet --help
echo "\`\`\`"
echo ""

commands=(address airdrop balance cancel confirm deploy get-transaction-count pay send-signature send-timestamp)

for x in "${commands[@]}"; do
    echo "\`\`\`manpage"
    soros-wallet "${x}" --help
    echo "\`\`\`"
    echo ""
done

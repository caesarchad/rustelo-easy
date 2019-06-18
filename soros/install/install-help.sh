#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/..

cargo build --package soros-install
export PATH=$PWD/target/debug:$PATH

echo "\`\`\`manpage"
soros-install --help
echo "\`\`\`"
echo ""

commands=(init info deploy update run)

for x in "${commands[@]}"; do
    echo "\`\`\`manpage"
    soros-install "${x}" --help
    echo "\`\`\`"
    echo ""
done

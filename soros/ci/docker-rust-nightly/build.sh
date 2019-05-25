#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

nightlyDate=${1:-$(date +%Y-%m-%d)}
docker build -t bitconch/rust-nightly:"$nightlyDate" --build-arg date="$nightlyDate" .

maybeEcho=
if [[ -z $CI ]]; then
  echo "Not CI, skipping |docker push|"
  maybeEcho="echo"
fi
$maybeEcho docker push bitconch/rust-nightly:"$nightlyDate"

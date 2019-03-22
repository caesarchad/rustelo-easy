#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"
eval "$(../../ci/channel-info.sh)"

if [[ -z $CHANNEL ]]; then
  echo Unable to determine channel to publish into, exiting.
  echo "^^^ +++"
  exit 0
fi

rm -rf usr/
../../ci/docker-run.sh bitconchlabs/rust:1.31.0 \
  scripts/cargo-install-all.sh sdk/docker-bitconch/usr

cp -f ../../run.sh usr/bin/bitconch-run.sh

docker build -t bitconchlabs/bitconch:"$CHANNEL" .

maybeEcho=
if [[ -z $CI ]]; then
  echo "Not CI, skipping |docker push|"
  maybeEcho="echo"
else
  (
    set +x
    if [[ -n $DOCKER_PASSWORD && -n $DOCKER_USERNAME ]]; then
      echo "$DOCKER_PASSWORD" | docker login --username "$DOCKER_USERNAME" --password-stdin
    fi
  )
fi
$maybeEcho docker push bitconchlabs/bitconch:"$CHANNEL"

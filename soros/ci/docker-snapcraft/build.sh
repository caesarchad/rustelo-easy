#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

docker build -t bitconchlabs/snapcraft .
docker push bitconchlabs/snapcraft

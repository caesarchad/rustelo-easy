#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

docker build -t bitconchlabs/rust .

read -r rustc version _ < <(docker run bitconchlabs/rust rustc --version)
[[ $rustc = rustc ]]
docker tag bitconchlabs/rust:latest bitconchlabs/rust:"$version"

docker push bitconchlabs/rust

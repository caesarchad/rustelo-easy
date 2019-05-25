#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

docker build -t bitconch/rust .

read -r rustc version _ < <(docker run bitconch/rust rustc --version)
[[ $rustc = rustc ]]
docker tag bitconch/rust:latest bitconch/rust:"$version"

docker push bitconch/rust

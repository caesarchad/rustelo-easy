#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

DRYRUN=
if [[ -z $BUILDKITE_BRANCH ]]; then
  DRYRUN="echo"
  CHANNEL=unknown
fi

eval "$(ci/channel-info.sh)"

TAG=
if [[ -n "$BUILDKITE_TAG" ]]; then
  CHANNEL_OR_TAG=$BUILDKITE_TAG
  TAG="$BUILDKITE_TAG"
elif [[ -n "$TRIGGERED_BUILDKITE_TAG" ]]; then
  CHANNEL_OR_TAG=$TRIGGERED_BUILDKITE_TAG
  TAG="$TRIGGERED_BUILDKITE_TAG"
else
  CHANNEL_OR_TAG=$CHANNEL
fi

if [[ -z $CHANNEL_OR_TAG ]]; then
  echo Unable to determine channel to publish into, exiting.
  exit 1
fi

case "$(uname)" in
Darwin)
  TARGET=x86_64-apple-darwin
  ;;
Linux)
  TARGET=x86_64-unknown-linux-gnu
  ;;
*)
  TARGET=unknown-unknown-unknown
  ;;
esac

echo --- Creating tarball
(
  set -x
  rm -rf soros-release/
  mkdir soros-release/

  COMMIT="$(git rev-parse HEAD)"

  (
    echo "channel: $CHANNEL"
    echo "commit: $COMMIT"
    echo "target: $TARGET"
  ) > soros-release/version.yml

  source ci/rust-version.sh stable
  scripts/cargo-install-all.sh +"$rust_stable" soros-release

  ./fetch-perf-libs.sh
  # shellcheck source=/dev/null
  source ./target/perf-libs/env.sh
  (
    cd fullnode
    cargo install --path . --features=cuda --root ../soros-release-cuda
  )
  cp soros-release-cuda/bin/soros-fullnode soros-release/bin/soros-fullnode-cuda
  cp -a scripts multinode-demo soros-release/

  tar jvcf soros-release-$TARGET.tar.bz2 soros-release/
)

echo --- Saving build artifacts
source ci/upload-ci-artifact.sh
upload-ci-artifact soros-release-$TARGET.tar.bz2

if [[ -n $DO_NOT_PUBLISH_TAR ]]; then
  echo Skipped due to DO_NOT_PUBLISH_TAR
  exit 0
fi

file=soros-release-$TARGET.tar.bz2
echo --- AWS S3 Store: $file
(
  set -x
  $DRYRUN docker run \
    --rm \
    --env AWS_ACCESS_KEY_ID \
    --env AWS_SECRET_ACCESS_KEY \
    --volume "$PWD:/soros" \
    eremite/aws-cli:2018.12.18 \
    /usr/bin/s3cmd --acl-public put /soros/"$file" s3://soros-release/"$CHANNEL_OR_TAG"/"$file"

  echo Published to:
  $DRYRUN ci/format-url.sh http://soros-release.s3.amazonaws.com/"$CHANNEL_OR_TAG"/"$file"
)

if [[ -n $TAG ]]; then
  ci/upload-github-release-asset.sh $file
fi

echo --- ok

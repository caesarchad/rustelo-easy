# |source| this file
#
# Common utilities shared by other scripts in this directory
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

rsync=rsync
bootstrap_leader_logger="tee bootstrap-leader.log"
fullnode_logger="tee fullnode.log"
drone_logger="tee drone.log"

if [[ $(uname) != Linux ]]; then
  # Protect against unsupported configurations to prevent non-obvious errors
  # later. Arguably these should be fatal errors but for now prefer tolerance.
  if [[ -n $USE_SNAP ]]; then
    echo "Warning: Snap is not supported on $(uname)"
    USE_SNAP=
  fi
  if [[ -n $BITCONCH_CUDA ]]; then
    echo "Warning: CUDA is not supported on $(uname)"
    BITCONCH_CUDA=
  fi
fi

if [[ -d $SNAP ]]; then # Running inside a Linux Snap?
  bitconch_program() {
    declare program="$1"
    printf "%s/command-%s.wrapper" "$SNAP" "$program"
  }
  rsync="$SNAP"/bin/rsync
  multilog="$SNAP/bin/multilog t s16777215 n200"
  bootstrap_leader_logger="$multilog $SNAP_DATA/bootstrap-leader"
  fullnode_logger="$multilog t $SNAP_DATA/fullnode"
  drone_logger="$multilog $SNAP_DATA/drone"
  # Create log directories manually to prevent multilog from creating them as
  # 0700
  mkdir -p "$SNAP_DATA"/{drone,bootstrap-leader,fullnode}

elif [[ -n $USE_SNAP ]]; then # Use the Linux Snap binaries
  bitconch_program() {
    declare program="$1"
    printf "bitconch.%s" "$program"
  }
elif [[ -n $USE_INSTALL ]]; then # Assume |./scripts/cargo-install-all.sh| was run
  bitconch_program() {
    declare program="$1"
    printf "bitconch-%s" "$program"
  }
else
  bitconch_program() {
    declare program="$1"
    declare features=""
    if [[ "$program" =~ ^(.*)-cuda$ ]]; then
      program=${BASH_REMATCH[1]}
      features="--features=cuda"
    fi

    if [[ -r "$(dirname "${BASH_SOURCE[0]}")"/../"$program"/Cargo.toml ]]; then
      maybe_package="--package bitconch-$program"
    fi
    if [[ -n $NDEBUG ]]; then
      maybe_release=--release
    fi
    printf "cargo run $maybe_release $maybe_package --bin bitconch-%s %s -- " "$program" "$features"
  }
  if [[ -n $BITCONCH_CUDA ]]; then
    # shellcheck disable=2154 # 'here' is referenced but not assigned
    if [[ -z $here ]]; then
      echo "|here| is not defined"
      exit 1
    fi

    # Locate perf libs downloaded by |./fetch-perf-libs.sh|
    LD_LIBRARY_PATH=$(cd "$here" && dirname "$PWD"/target/perf-libs):$LD_LIBRARY_PATH
    export LD_LIBRARY_PATH
  fi
fi

bitconch_bench_tps=$(bitconch_program bench-tps)
bitconch_wallet=$(bitconch_program wallet)
bitconch_drone=$(bitconch_program drone)
bitconch_fullnode=$(bitconch_program fullnode)
bitconch_fullnode_config=$(bitconch_program fullnode-config)
bitconch_fullnode_cuda=$(bitconch_program fullnode-cuda)
bitconch_genesis=$(bitconch_program genesis)
bitconch_keygen=$(bitconch_program keygen)
bitconch_ledger_tool=$(bitconch_program ledger-tool)

export RUST_LOG=${RUST_LOG:-bitconch=info} # if RUST_LOG is unset, default to info
export RUST_BACKTRACE=1

# shellcheck source=scripts/configure-metrics.sh
source "$(dirname "${BASH_SOURCE[0]}")"/../scripts/configure-metrics.sh

tune_system() {
  # Skip in CI
  [[ -z $CI ]] || return 0

  # shellcheck source=scripts/ulimit-n.sh
  source "$(dirname "${BASH_SOURCE[0]}")"/../scripts/ulimit-n.sh

  # Reference: https://medium.com/@CameronSparr/increase-os-udp-buffers-to-improve-performance-51d167bb1360
  if [[ $(uname) = Linux ]]; then
    (
      set -x +e
      # test the existence of the sysctls before trying to set them
      # go ahead and return true and don't exit if these calls fail
      sysctl net.core.rmem_max 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.rmem_max=1610612736 1>/dev/null 2>/dev/null

      sysctl net.core.rmem_default 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.rmem_default=1610612736 1>/dev/null 2>/dev/null

      sysctl net.core.wmem_max 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.wmem_max=1610612736 1>/dev/null 2>/dev/null

      sysctl net.core.wmem_default 2>/dev/null 1>/dev/null &&
          sudo sysctl -w net.core.wmem_default=1610612736 1>/dev/null 2>/dev/null
    ) || true
  fi

  if [[ $(uname) = Darwin ]]; then
    (
      if [[ $(sysctl net.inet.udp.maxdgram | cut -d\  -f2) != 65535 ]]; then
        echo "Adjusting maxdgram to allow for large UDP packets, see BLOB_SIZE in src/packet.rs:"
        set -x
        sudo sysctl net.inet.udp.maxdgram=65535
      fi
    )

  fi
}

# The directory on the bootstrap leader that is rsynced by other full nodes as
# they boot (TODO: Eventually this should go away)
BITCONCH_RSYNC_CONFIG_DIR=${SNAP_DATA:-$PWD}/config

# Configuration that remains local
BITCONCH_CONFIG_DIR=${SNAP_DATA:-$PWD}/config-local

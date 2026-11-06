#!/bin/sh
# Run a command inside a throwaway Rust container with the repository mounted. No Rust or Solana tooling is
# needed on the host. The cargo registry and the target directory live in named docker volumes, so nothing
# lands on the host except what the command writes into the repository (Cargo.lock, formatted sources).
#
# usage: scripts/rs.sh '<shell command>'          offline run (network none): build, test, clippy, fmt
#        scripts/rs.sh --net '<shell command>'    with network; accepted: `cargo fetch --locked`, and for lockfile
#                                                 maintenance `cargo generate-lockfile` (no Cargo.lock yet) and
#                                                 `cargo update -p NAME@VERSION --precise VERSION` (one crate,
#                                                 used to step a transitive crate back below the age cutoff;
#                                                 never a blanket `cargo update`)
#
# The container runs as the calling (non-root) user, drops every capability, forbids privilege gain and is
# limited to 4 CPUs and 8 GiB. The machine is shared: run one heavy job at a time.
#
# Image: the official `rust` image pinned by digest (never by tag): Rust 1.95.0, built 2026-05-20.
# Override with RS_IMAGE only to try another pinned digest.
set -eu

image=${RS_IMAGE:-rust@sha256:d7482085ff5b415f84dba5647ae71606650bdef00db7aeb69f4b3d170c3e4082}

net=none
if [ "${1:-}" = "--net" ]; then
  net=bridge
  shift
  case "${1:-}" in
    "cargo fetch --locked"*) ;;
    "cargo generate-lockfile") [ ! -e "$(dirname "$0")/../Cargo.lock" ] || { echo "rs.sh: Cargo.lock exists" >&2; exit 2; } ;;
    "cargo update -p "*" --precise "*) ;;
    *) echo "rs.sh: --net accepts only cargo fetch --locked, cargo generate-lockfile, cargo update -p X --precise V" >&2; exit 2 ;;
  esac
fi
[ "$#" -eq 1 ] || { echo "usage: rs.sh [--net] '<shell command>'" >&2; exit 2; }

repo=$(cd "$(dirname "$0")/.." && pwd)
name=$(basename "$repo")
cvol="$name-cargo"; tvol="$name-target"
docker volume create "$cvol" > /dev/null
docker volume create "$tvol" > /dev/null
# volumes are created root-owned; hand them to the calling user once (this one step needs no network and exits)
docker run --rm --network none -u 0 -v "$cvol:/c" -v "$tvol:/t" "$image" chown "$(id -u):$(id -g)" /c /t > /dev/null

exec docker run --rm \
  --user "$(id -u):$(id -g)" \
  --cap-drop ALL --security-opt no-new-privileges \
  --cpus 4 --memory 8g --pids-limit 4096 \
  --network "$net" \
  -e HOME=/tmp/home -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/target -e CARGO_TERM_COLOR=never \
  -e CARGO_NET_OFFLINE="$([ "$net" = none ] && echo true || echo false)" \
  -v "$repo:/work" -v "$cvol:/cargo" -v "$tvol:/target" -w /work \
  "$image" sh -c "$1"

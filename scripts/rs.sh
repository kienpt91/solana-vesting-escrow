#!/bin/sh
# Run a command inside a throwaway Rust container with the repository mounted. No Rust or Solana tooling is
# needed on the host. The cargo registry, the rustup home and the target directory live in named docker volumes,
# so nothing lands on the host except what the command writes into the repository (Cargo.lock, formatted sources).
#
# usage: scripts/rs.sh '<shell command>'          offline run (network none): build, test, clippy, fmt
#        scripts/rs.sh --net '<shell command>'    with network; only these commands are accepted:
#          cargo fetch --locked                   download the locked crates (the only crates.io access)
#          rustup component add ...               one-time: clippy, rustfmt and llvm-tools for the pinned
#                                                 toolchain 1.95.0 (the image ships the minimal profile)
#          cargo generate-lockfile                only while there is no Cargo.lock yet
#          cargo update -p NAME@VERSION --precise VERSION
#                                                 one crate, to step a transitive crate back below the age
#                                                 cutoff; never a blanket `cargo update`
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
    "rustup component add "*) ;;
    "cargo generate-lockfile") [ ! -e "$(dirname "$0")/../Cargo.lock" ] || { echo "rs.sh: Cargo.lock exists" >&2; exit 2; } ;;
    "cargo update -p "*" --precise "*) ;;
    *) echo "rs.sh: --net accepts only cargo fetch --locked, rustup component add, cargo generate-lockfile, cargo update -p X --precise V" >&2; exit 2 ;;
  esac
fi
[ "$#" -eq 1 ] || { echo "usage: rs.sh [--net] '<shell command>'" >&2; exit 2; }

repo=$(cd "$(dirname "$0")/.." && pwd)
name=$(basename "$repo")
cvol="$name-cargo"; tvol="$name-target"; rvol="$name-rustup"
for v in "$cvol" "$tvol" "$rvol"; do docker volume create "$v" > /dev/null; done
# volumes are created root-owned: hand them to the calling user once and seed the rustup home from the image
# (this step needs no network and exits)
docker run --rm --network none -u 0 -v "$cvol:/c" -v "$tvol:/t" -v "$rvol:/r" "$image" sh -c '
  chown "$1:$2" /c /t
  if [ ! -e /r/settings.toml ]; then cp -a /usr/local/rustup/. /r/ && chown -R "$1:$2" /r; fi' sh "$(id -u)" "$(id -g)"

exec docker run --rm \
  --user "$(id -u):$(id -g)" \
  --cap-drop ALL --security-opt no-new-privileges \
  --cpus 4 --memory 8g --pids-limit 4096 \
  --network "$net" \
  -e HOME=/tmp/home -e CARGO_HOME=/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR=/target -e CARGO_TERM_COLOR=never \
  -e CARGO_NET_OFFLINE="$([ "$net" = none ] && echo true || echo false)" \
  -v "$repo:/work" -v "$cvol:/cargo" -v "$tvol:/target" -v "$rvol:/rustup" -w /work \
  "$image" sh -c "$1"

#!/bin/sh
# Proves that Cargo.lock is consistent, from nothing:
#   1. the static checks of scripts/check-lock.sh (sources, checksums present, no patches)
#   2. `cargo fetch --locked` into a THROWAWAY cargo volume: every crate is downloaded again and cargo compares the SHA-256 of
#      the download with the checksum in Cargo.lock, so a hand-edited or wrong checksum fails here (a cached download would not)
#   3. `cargo metadata --locked --offline` on that volume: the lock file resolves the whole workspace without changes
#   4. the age check of scripts/check-crates-age.mjs (needs node and crates.io; skipped with LOCK_AGE=skip)
# Needs docker; the network is used by step 2 and 4 only. About 15 minutes, almost all of it step 4 (crates.io asks for one
# request per second). usage: scripts/verify-lock.sh
#   LOCK_AGE_CUTOFF   the age cutoff (default: the one of this repository, 2026-10-29T00:00:00Z)
#   LOCK_AGE=skip     skip step 4
set -eu
cd "$(dirname "$0")/.."
name=$(basename "$(pwd)")
vol="$name-cargo-verify-$$"
trap 'docker volume rm -f "$vol" > /dev/null 2>&1 || true' EXIT INT TERM

echo "== 1. static checks =="
sh scripts/check-lock.sh
echo "== 2. fetch every locked crate into a fresh cargo volume (checksums are verified by cargo) =="
RS_CARGO_VOLUME=$vol sh scripts/rs.sh --net 'cargo fetch --locked'
echo "== 3. the lock file resolves the workspace unchanged =="
RS_CARGO_VOLUME=$vol sh scripts/rs.sh 'cargo metadata --locked --offline --format-version 1 > /dev/null && echo "cargo metadata --locked: ok" && cargo tree --locked --offline --workspace --prefix none | sed "s/ (.*//" | sort -u | wc -l | sed "s/^/unique crates in the resolved tree: /"'
if [ "${LOCK_AGE:-}" = skip ]; then
  echo "== 4. age check skipped =="
else
  echo "== 4. age check =="
  node scripts/check-crates-age.mjs . "${LOCK_AGE_CUTOFF:-2026-10-29T00:00:00Z}"
fi
echo "verify-lock: ok"

#!/bin/sh
# Runs the test suites layer by layer and prints a count per layer. Run it where the repository is mounted and the
# dependencies are fetched, for example through the container wrapper:  scripts/rs.sh 'sh scripts/test-layers.sh'
#
#   core          unit, worked-example and seeded property tests of the pure schedule logic (vesting_core)
#   program       byte-level contracts of the program crate: account layout, instruction data, error table, addresses
#   client        the instruction builders (account order, signer and writable flags, data)
#   happy paths   every instruction on its intended path, on the in-process bank
#   edge cases    boundary values and schedule bounds, on the in-process bank
#   harness       the test harness itself: error labels and the numbering of the attack rows
#   attacks       hostile transactions, each expecting one precise error and no state change
#   architecture  the dependency and purity rules checked on manifests, lock file and sources
set -eu
cd "$(dirname "$0")/.."
OPTS="--locked --offline"
passed_all=0

layer() {
  name=$1; shift
  printf '%-14s ' "$name"
  out=$("$@" 2>&1) || { printf '\n%s\n' "$out"; echo "FAILED: $name" >&2; exit 1; }
  counts=$(printf '%s\n' "$out" | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "%d %d %d", p, f, i}')
  set -- $counts
  printf '%d passed, %d failed, %d ignored\n' "$1" "$2" "$3"
  passed_all=$((passed_all + $1))
}

layer "core"         cargo test $OPTS -p vesting_core
layer "program"      cargo test $OPTS -p vesting_escrow
layer "client"       cargo test $OPTS -p vesting_client
layer "happy paths"  cargo test $OPTS -p vesting_tests --test happy_paths
layer "edge cases"   cargo test $OPTS -p vesting_tests --test edge_cases
layer "harness"      cargo test $OPTS -p vesting_tests --test harness
layer "attacks"      cargo test $OPTS -p vesting_tests --test attacks
layer "architecture" cargo test $OPTS -p vesting_tests --test architecture
printf '%-14s %d passed\n' "total" "$passed_all"

#!/bin/sh
# Prints, from a real run: the vesting curve of an example schedule, a lifecycle on the in-process bank, and the attack
# table (one row per hostile transaction: the error expected, the error observed, the verdict).
# Run it through the container wrapper:  scripts/rs.sh 'sh scripts/demo.sh' > docs/demo-output.txt
# Nothing here talks to a cluster; every transaction runs on an in-process bank (see README, "What the tests execute").
#
# The demo fails (non-zero exit) when cargo fails or when the attack table cannot be trusted: see scripts/attack-table.sh.
set -eu
cd "$(dirname "$0")/.."
export RUST_LOG=error
OPTS="--locked --offline --quiet"

echo "== 1. the vesting curve (crates/vesting_core/examples/curve.rs) =="
cargo run $OPTS -p vesting_core --example curve
echo
echo "== 2. a lifecycle on the in-process bank (crates/vesting_tests/examples/walkthrough.rs) =="
cargo run $OPTS -p vesting_tests --example walkthrough 2>/dev/null
echo
echo "== 3. the attack table (crates/vesting_tests/tests/attacks.rs) =="
raw=$(mktemp)
trap 'rm -f "$raw"' EXIT INT TERM
# the exit status of cargo is kept: it is not the status of a pipeline's last command
status=0
cargo test $OPTS -p vesting_tests --test attacks -- --nocapture --test-threads=1 > "$raw" 2>&1 || status=$?
sh scripts/attack-table.sh "$raw" "$status"

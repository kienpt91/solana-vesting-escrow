#!/bin/sh
# Prints, from a real run: the vesting curve of an example schedule, a lifecycle on the in-process bank, and the attack
# table (one row per hostile transaction: the error expected, the error observed, the verdict).
# Run it through the container wrapper:  scripts/rs.sh 'sh scripts/demo.sh' > docs/demo-output.txt
# Nothing here talks to a cluster; every transaction runs on an in-process bank (see README, "What the tests execute").
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
trap 'rm -f "$raw"' EXIT
cargo test $OPTS -p vesting_tests --test attacks -- --nocapture --test-threads=1 2>&1 | grep '^ATTACK|' | sort > "$raw" || true
awk -F'|' '
  { n++; if ($5 == "PASS") pass++; else fail++
    printf "%3d  %-4s  %s\n       expected: %s\n       observed: %s\n", n, $5, $2, $3, $4 }
  END { printf "\n%d hostile transactions, %d rejected exactly as expected, %d not\n", n, pass, fail; if (fail > 0 || n == 0) exit 1 }
' "$raw"

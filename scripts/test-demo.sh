#!/bin/sh
# Shell tests of scripts/attack-table.sh, the checker behind the attack table of the demo. The fixtures imitate the raw
# output of `cargo test -- --nocapture`, including the way libtest glues "test NAME ... " in front of the first line a
# test prints. Needs only a POSIX shell. usage: sh scripts/test-demo.sh
set -eu
scripts=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

passed=0
failed=0
ok() { passed=$((passed + 1)); }
bad() { failed=$((failed + 1)); echo "FAIL: $1" >&2; }
table() { sh "$scripts/attack-table.sh" "$@" > "$work/out" 2>&1; }
expect_ok() { desc=$1; shift; if table "$@"; then ok; else bad "$desc"; sed 's/^/  /' "$work/out" >&2; fi; }
expect_fail() { desc=$1; shift; if table "$@"; then bad "$desc (accepted)"; else ok; fi; }
expect_in() { desc=$1; needle=$2; if grep -qF -- "$needle" "$work/out"; then ok; else bad "$desc (missing: $needle)"; sed 's/^/  /' "$work/out" >&2; fi; }

# a clean run: the first row of a test is glued to libtest's "test NAME ... " prefix, the others stand alone
cat > "$work/good" <<'RAW'
running 3 tests
test alpha ... ATTACK|1|first attack|instruction 0: A (1)|instruction 0: A (1)|PASS
test beta ... ATTACK|2|second attack|instruction 0: B (2)|instruction 0: B (2)|PASS
ATTACK|3|third attack|instruction 1: C (3)|instruction 1: C (3)|PASS
ok
ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s
RAW
expect_ok "accepts a clean run, including rows glued to the libtest prefix" "$work/good" 0
expect_in "counts the glued first rows" "3 hostile transactions, 3 rejected exactly as expected, 0 not"
expect_in "prints the glued first row" "first attack"
expect_in "prints the expected and the observed error" "expected: instruction 0: A (1)"

# a failing cargo status must fail the demo even though every printed row says PASS
expect_fail "refuses a non-zero cargo status" "$work/good" 101
expect_in "names the failed status" "cargo test failed"

# a dropped row leaves a gap in the sequence numbers
sed '/second attack/d' "$work/good" > "$work/gap"
expect_fail "refuses a gap in the sequence" "$work/gap" 0
expect_in "names the gap" "missing"

# the same row twice
{ cat "$work/good"; echo "ATTACK|3|third attack|instruction 1: C (3)|instruction 1: C (3)|PASS"; } > "$work/dup"
expect_fail "refuses a repeated row" "$work/dup" 0

# a row that says FAIL
sed 's/|PASS$/|FAIL/' "$work/good" > "$work/fail"
expect_fail "refuses a FAIL verdict" "$work/fail" 0
expect_in "names the verdict" "not rejected exactly as expected"

# no rows at all
printf 'running 0 tests\ntest result: ok. 0 passed; 0 failed\n' > "$work/none"
expect_fail "refuses an output without rows" "$work/none" 0

# fewer rows than tests that passed: every attack test prints at least one row
sed 's/2 passed/9 passed/' "$work/good" > "$work/few"
expect_fail "refuses fewer rows than passing tests" "$work/few" 0
expect_in "names the undercount" "fewer rows than"

# no libtest summary line: the run did not finish
sed '/^test result/d' "$work/good" > "$work/nosum"
expect_fail "refuses an output without the libtest summary" "$work/nosum" 0

# missing arguments
expect_fail "refuses missing arguments" "$work/good"

echo "demo scripts: $passed checks passed"
[ "$failed" -eq 0 ]

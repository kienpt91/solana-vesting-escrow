#!/bin/sh
# Turns the raw output of `cargo test -p vesting_tests --test attacks -- --nocapture` into the attack table, and fails
# when the output cannot be trusted: a non-zero cargo status, no rows, a gap or a repeat in the row numbers, a row whose
# verdict is not PASS, fewer rows than passing tests, or no libtest summary line.
#
# Every hostile transaction prints one row, `ATTACK|seq|name|expected|observed|verdict`, numbered 1, 2, 3 ... in the order
# the tests ran. libtest glues "test NAME ... " in front of the first thing a test prints, so rows are cut out of the line
# with grep -o instead of being matched at the start of it.
#
# usage: attack-table.sh RAW_OUTPUT_FILE CARGO_EXIT_STATUS
set -eu
[ "$#" -eq 2 ] || { echo "usage: attack-table.sh RAW_OUTPUT_FILE CARGO_EXIT_STATUS" >&2; exit 2; }
raw=$1
status=$2

if [ "$status" != 0 ]; then
  echo "attack table: cargo test failed (exit status $status); the table would not be trustworthy" >&2
  exit 1
fi

rows=$(mktemp)
trap 'rm -f "$rows"' EXIT INT TERM
grep -a -o 'ATTACK|.*$' "$raw" | sort -t'|' -k2,2n > "$rows" || true

count=$(wc -l < "$rows" | tr -d ' ')
if [ "$count" -eq 0 ]; then
  echo "attack table: no ATTACK rows in the output" >&2
  exit 1
fi

# the numbers must be exactly 1..count: a drop or a repeat shows up as a gap or a duplicate
if ! awk -F'|' '
  { n++; if ($2 + 0 != n) { printf "attack table: row number %d is missing or repeated (found %s where %d was expected)\n", n, $2, n > "/dev/stderr"; bad = 1; exit } }
  END { exit bad ? 1 : 0 }
' "$rows"; then
  echo "attack table: rows are missing from the output (gap or repeat in the numbering)" >&2
  exit 1
fi

summary=$(grep -a -E '^test result: ' "$raw" | tail -n 1 || true)
if [ -z "$summary" ]; then
  echo "attack table: the libtest summary line is missing, the run did not finish" >&2
  exit 1
fi
tests=$(printf '%s\n' "$summary" | sed -n 's/^test result: [a-zA-Z]*\. \([0-9][0-9]*\) passed.*/\1/p')
if [ -z "$tests" ] || [ "$count" -lt "$tests" ]; then
  echo "attack table: fewer rows than passing tests ($count rows, ${tests:-?} tests; every attack test prints at least one row)" >&2
  exit 1
fi

awk -F'|' '
  { n++; if ($6 == "PASS") pass++; else fail++
    printf "%3d  %-4s  %s\n       expected: %s\n       observed: %s\n", n, $6, $3, $4, $5 }
  END {
    printf "\n%d hostile transactions, %d rejected exactly as expected, %d not\n", n, pass, fail
    if (fail > 0) { print "attack table: some transactions were not rejected exactly as expected" > "/dev/stderr"; exit 1 }
  }
' "$rows"

#!/bin/sh
# Line coverage per crate with rustc's source-based instrumentation (`-C instrument-coverage`) and the llvm-profdata and
# llvm-cov tools of the `llvm-tools` rustup component. No third-party coverage crate. Run it through the container wrapper
# (the component is added once with `scripts/rs.sh --net 'rustup component add llvm-tools-preview'`):
#
#   scripts/rs.sh 'sh scripts/coverage.sh'
#
# What is counted: the library code of vesting_core, vesting_escrow and vesting_client, as executed by every test of the
# workspace. The vesting program is compiled natively into the test binaries, so its lines are measured while the in-process
# bank runs it. Not counted: files under tests/ and examples/, the test harness crate, dependencies, and the generated
# on-chain entrypoint (compiled out of the tests by the `no-entrypoint` feature).
set -eu
cd "$(dirname "$0")/.."
SYSROOT=$(rustc --print sysroot)
HOST=$(rustc -vV | sed -n 's/^host: //p')
BIN="$SYSROOT/lib/rustlib/$HOST/bin"
COV=${COV_DIR:-/target/cov}
rm -rf "$COV"
mkdir -p "$COV/raw"
export RUSTFLAGS="-C instrument-coverage"
export CARGO_TARGET_DIR="$COV/build"
export LLVM_PROFILE_FILE="$COV/raw/p-%p-%m.profraw"
# the linker of an instrumented test binary that embeds the whole Solana runtime needs a lot of memory: no debug info
# (coverage reads the coverage map, not DWARF) and two jobs at a time keep it inside 8 GiB
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_BUILD_JOBS=2

cargo test --workspace --tests --locked --offline --no-fail-fast
objects=$(cargo test --workspace --tests --locked --offline --no-run --message-format=json 2>/dev/null \
  | grep -o '"executable":"[^"]*"' | cut -d'"' -f4 | sort -u)

"$BIN/llvm-profdata" merge -sparse "$COV"/raw/*.profraw -o "$COV/all.profdata"
first=""; rest=""
for o in $objects; do
  if [ -z "$first" ]; then first=$o; else rest="$rest -object=$o"; fi
done
# shellcheck disable=SC2086
"$BIN/llvm-cov" export --format=lcov --instr-profile="$COV/all.profdata" \
  --ignore-filename-regex='(/\.cargo/|/cargo/registry/|/rustc/|/tests/|/examples/|crates/vesting_tests/|/build/[^/]*/out/)' \
  "$first" $rest > "$COV/lcov.info"

awk '
  /^SF:/ {
    file = substr($0, 4); crate = "other"
    if (file ~ /crates\/vesting_core\//) crate = "vesting_core (pure logic)"
    else if (file ~ /programs\/vesting_escrow\//) crate = "vesting_escrow (program)"
    else if (file ~ /crates\/vesting_client\//) crate = "vesting_client (SDK)"
    next
  }
  /^DA:/ { split(substr($0, 4), da, ","); total[crate]++; if (da[2] + 0 > 0) covered[crate]++ }
  END {
    for (c in total) { printf "%-28s %8d %8d %7.1f%%\n", c, total[c], covered[c], 100 * covered[c] / total[c]; all += total[c]; allc += covered[c] }
    printf "%-28s %8d %8d %7.1f%%\n", "all counted code", all, allc, 100 * allc / all
  }
' "$COV/lcov.info" | sort > "$COV/rows.txt"
{ printf "%-28s %8s %8s %8s\n" "crate" "lines" "covered" "percent"; cat "$COV/rows.txt"; } | tee "$COV/summary.txt"
# per-file detail for the program crate
awk '
  /^SF:/ { file = substr($0, 4); next }
  /^DA:/ { split(substr($0, 4), da, ","); t[file]++; if (da[2] + 0 > 0) c[file]++ }
  END { for (f in t) if (f ~ /programs\/vesting_escrow\//) printf "  %-60s %4d/%-4d %6.1f%%\n", f, c[f], t[f], 100 * c[f] / t[f] }
' "$COV/lcov.info" | sort | sed 's#/work/##'

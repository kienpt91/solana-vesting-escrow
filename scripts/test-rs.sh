#!/bin/sh
# Shell tests of the --net allow list of scripts/rs.sh. A stub `docker` on the PATH records what the wrapper would run, so
# the tests need neither docker nor network. usage: sh scripts/test-rs.sh
set -eu
scripts=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM
mkdir "$work/bin"
cat > "$work/bin/docker" <<'STUB'
#!/bin/sh
echo "docker $*" >> "$DOCKER_LOG"
exit 0
STUB
chmod +x "$work/bin/docker"
export DOCKER_LOG="$work/docker.log"

passed=0
failed=0
ok() { passed=$((passed + 1)); }
bad() { failed=$((failed + 1)); echo "FAIL: $1" >&2; }
run() { : > "$DOCKER_LOG"; PATH="$work/bin:$PATH" sh "$scripts/rs.sh" "$@" > "$work/out" 2>&1; }
expect_refused() {
  desc=$1; shift
  if run "$@"; then bad "$desc (accepted)"; return; fi
  if [ -s "$DOCKER_LOG" ]; then bad "$desc (docker was called)"; else ok; fi
}
expect_runs_with_network() {
  desc=$1; shift
  if ! run "$@"; then bad "$desc (refused)"; sed 's/^/  /' "$work/out" >&2; return; fi
  if grep -q -- '--network bridge' "$DOCKER_LOG"; then ok; else bad "$desc (no network flag)"; fi
}

expect_runs_with_network "allows cargo fetch --locked" --net 'cargo fetch --locked'
expect_runs_with_network "allows adding the toolchain components" --net 'rustup component add clippy rustfmt llvm-tools-preview'
expect_runs_with_network "allows stepping one crate back" --net 'cargo update -p cc@1.11.0 --precise 1.10.0'

# the offline path never gets the network
run 'cargo test --locked --offline'
if grep -q -- '--network none' "$DOCKER_LOG" && ! grep -q -- '--network bridge' "$DOCKER_LOG"; then ok; else bad "an offline command must run with network none"; fi
if grep -q -- '--cap-drop ALL' "$DOCKER_LOG" && grep -q -- '--cpus 4' "$DOCKER_LOG" && grep -q -- '--memory 8g' "$DOCKER_LOG"; then ok; else bad "the hardening flags are missing"; fi

# chained or otherwise extended commands are refused before docker is reached
expect_refused "refuses a command chained with ;" --net 'cargo fetch --locked; echo owned'
expect_refused "refuses a command chained with &&" --net 'cargo fetch --locked && echo owned'
expect_refused "refuses a pipe" --net 'cargo fetch --locked | sh'
expect_refused "refuses a command substitution" --net 'cargo fetch --locked $(echo owned)'
expect_refused "refuses a backtick" --net 'cargo fetch --locked `echo owned`'
expect_refused "refuses a redirection" --net 'cargo fetch --locked > /tmp/x'
expect_refused "refuses a newline" --net 'cargo fetch --locked
echo owned'
expect_refused "refuses a chain behind rustup component add" --net 'rustup component add clippy; curl http://example.invalid'
expect_refused "refuses a chain behind cargo update" --net 'cargo update -p cc@1.11.0 --precise 1.10.0; echo owned'
expect_refused "refuses a quote" --net "cargo fetch --locked 'x'"

# commands that are not on the list
expect_refused "refuses an unlisted command" --net 'curl http://example.invalid'
expect_refused "refuses a blanket cargo update" --net 'cargo update'
expect_refused "refuses cargo update without --precise" --net 'cargo update -p cc@1.11.0'
expect_refused "refuses cargo fetch with extra words" --net 'cargo fetch --locked --verbose'
expect_refused "refuses generate-lockfile while a lock file exists" --net 'cargo generate-lockfile'
expect_refused "refuses an empty command" --net ''
expect_refused "refuses two arguments" --net 'cargo fetch --locked' 'echo owned'

echo "rs.sh: $passed checks passed"
[ "$failed" -eq 0 ]

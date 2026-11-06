#!/bin/sh
# Shell tests of scripts/check-lock.sh against hand-made lock files. usage: sh scripts/test-lock.sh
set -eu
scripts=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

passed=0
failed=0
ok() { passed=$((passed + 1)); }
bad() { failed=$((failed + 1)); echo "FAIL: $1" >&2; }
n=0
sum() { printf '%064d' "$1" | tr 0 a; }  # a 64-digit hexadecimal string
newdir() { n=$((n + 1)); d="$work/d$n"; mkdir -p "$d"; printf '[workspace]\nmembers = []\n' > "$d/Cargo.toml"; }
check() { sh "$scripts/check-lock.sh" "$d" > "$work/out" 2>&1; }
expect_ok() { if check; then ok; else bad "$1"; sed 's/^/  /' "$work/out" >&2; fi; }
expect_fail() { if check; then bad "$1 (accepted)"; return; fi; if grep -qF -- "$2" "$work/out"; then ok; else bad "$1 (refused, but not for: $2)"; sed 's/^/  /' "$work/out" >&2; fi; }
registry='registry+https://github.com/rust-lang/crates.io-index'
package() { # name version source checksum
  printf '[[package]]\nname = "%s"\nversion = "%s"\n' "$1" "$2"
  [ -z "${3:-}" ] || printf 'source = "%s"\n' "$3"
  [ -z "${4:-}" ] || printf 'checksum = "%s"\n' "$4"
  printf '\n'
}

newdir
{ echo 'version = 4'; echo; package vesting_core 0.1.0; package serde 1.0.0 "$registry" "$(sum 1)"; package serde 2.0.0 "$registry" "$(sum 2)"; } > "$d/Cargo.lock"
expect_ok "accepts registry packages with checksums and a workspace crate"

newdir
{ echo 'version = 4'; echo; package serde 1.0.0 "git+https://example.invalid/serde#abc"; } > "$d/Cargo.lock"
expect_fail "refuses a git source" "not from crates.io"

newdir
{ echo 'version = 4'; echo; package serde 1.0.0 "registry+https://example.invalid/index" "$(sum 1)"; } > "$d/Cargo.lock"
expect_fail "refuses another registry" "not from crates.io"

newdir
{ echo 'version = 4'; echo; package serde 1.0.0 "$registry"; } > "$d/Cargo.lock"
expect_fail "refuses a missing checksum" "no valid checksum"

newdir
{ echo 'version = 4'; echo; package serde 1.0.0 "$registry" "abc123"; } > "$d/Cargo.lock"
expect_fail "refuses a short checksum" "no valid checksum"

newdir
{ echo 'version = 4'; echo; package serde 1.0.0 "$registry" "$(sum 1)"; package serde 1.0.0 "$registry" "$(sum 1)"; } > "$d/Cargo.lock"
expect_fail "refuses a repeated package" "duplicate package"

newdir
{ echo 'version = 4'; echo; package sneaky 0.1.0; } > "$d/Cargo.lock"
expect_fail "refuses a path package that is not a workspace crate" "not a workspace crate"

newdir
{ echo 'version = 4'; echo; package serde 1.0.0 "$registry" "$(sum 1)"; } > "$d/Cargo.lock"
printf '[patch.crates-io]\nserde = { path = "../serde" }\n' >> "$d/Cargo.toml"
expect_fail "refuses a [patch] section" "[patch] or [replace]"

newdir
{ echo 'version = 4'; echo; package serde 1.0.0 "$registry" "$(sum 1)"; } > "$d/Cargo.lock"
mkdir -p "$d/.cargo"
printf '[source.crates-io]\nreplace-with = "mirror"\n' > "$d/.cargo/config.toml"
expect_fail "refuses a cargo config that can redirect a source" "could redirect"

newdir
expect_fail "refuses a missing lock file" "no Cargo.lock"

echo "lock file checks: $passed checks passed"
[ "$failed" -eq 0 ]

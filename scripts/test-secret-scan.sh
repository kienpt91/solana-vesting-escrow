#!/bin/sh
# Shell tests of scripts/secret-scan.sh for Solana key material: the 64-number JSON array of a keypair file, a base58 string
# of the length of a 64-byte secret key or signature, and the usual keypair file names. Every fixture is built at run time in
# a throwaway repository, so no key-shaped text exists in this file. Needs git and a POSIX shell.
# usage: sh scripts/test-secret-scan.sh
set -eu
scripts=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=probe GIT_AUTHOR_EMAIL=probe@example.invalid GIT_COMMITTER_NAME=probe GIT_COMMITTER_EMAIL=probe@example.invalid

passed=0
failed=0
ok() { passed=$((passed + 1)); }
bad() { failed=$((failed + 1)); echo "FAIL: $1" >&2; }

n=0
newrepo() { # a fresh repository with a copy of the scanner; its directory is left in $r
  n=$((n + 1))
  r="$work/r$n"
  mkdir -p "$r/scripts"
  cp "$scripts/secret-scan.sh" "$r/scripts/"
  git -C "$r" init -q -b main
}
scan() { (cd "$r" && sh scripts/secret-scan.sh "$@") > "$work/out" 2>&1; }
expect_blocked() { # description, mode, rule
  if scan "$2"; then bad "$1 (not blocked)"; return; fi
  if grep -qF -- "$3" "$work/out"; then ok; else bad "$1 (blocked, but not by $3)"; sed 's/^/  /' "$work/out" >&2; fi
}
expect_clean() { if scan "$2"; then ok; else bad "$1 (blocked)"; sed 's/^/  /' "$work/out" >&2; fi; }

# a keypair file is 64 numbers in 0..255 in a JSON array
array() { awk -v n="$1" 'BEGIN { printf "["; for (i = 0; i < n; i++) printf "%s%d", (i ? "," : ""), (i * 37 + 11) % 256; printf "]\n" }'; }
# a base58 string of a given length (the alphabet has no 0, O, I or l)
b58() { awk -v n="$1" 'BEGIN { a = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"; for (i = 0; i < n; i++) printf "%s", substr(a, (i * 7) % 58 + 1, 1); printf "\n" }'; }

# 64 numbers in an array: blocked, in a file with any name and spaced or not
newrepo
array 64 > "$r/notes.txt"
git -C "$r" add -A
expect_blocked "blocks a keypair array in an ordinary file" --staged solana-keypair-array
newrepo
awk 'BEGIN { printf "[ "; for (i = 0; i < 64; i++) printf "%s%d", (i ? " , " : ""), (i * 5) % 256; printf " ]\n" }' > "$r/docs.md"
git -C "$r" add -A
expect_blocked "blocks a keypair array with spaces" --staged solana-keypair-array
newrepo
{ printf 'let secret = '; array 64 | tr -d '\n'; echo ';'; } > "$r/lib.rs"
git -C "$r" add -A
expect_blocked "blocks a keypair array inside source code" --staged solana-keypair-array

# 32 numbers (a public key or a seed written as bytes) and 63 numbers are not a keypair
newrepo
{ array 32; array 63; echo "[1, 2, 3]"; } > "$r/bytes.txt"
git -C "$r" add -A
expect_clean "allows 32 and 63 numbers" --staged

# a base58 string of 88 or 87 characters (a 64-byte secret key or a signature) in quotes
newrepo
printf 'key = "%s"\n' "$(b58 88)" > "$r/config.txt"
git -C "$r" add -A
expect_blocked "blocks an 88 character base58 string" --staged base58-64-byte-value
newrepo
printf "key = '%s'\n" "$(b58 87)" > "$r/config.txt"
git -C "$r" add -A
expect_blocked "blocks an 87 character base58 string" --staged base58-64-byte-value
# a public key (32 to 44 characters) is not secret
newrepo
printf 'program = "%s"\nother = "%s"\n' "$(b58 44)" "$(b58 32)" > "$r/config.txt"
git -C "$r" add -A
expect_clean "allows a public key" --staged

# keypair file names, at the root and in subdirectories
for name in id.json keypair.json deploy-keypair.json keypair-main.json; do
  newrepo
  mkdir -p "$r/deploy/keys"
  echo '{}' > "$r/deploy/keys/$name"
  git -C "$r" add -f -A
  expect_blocked "blocks a file named $name in a subdirectory" --staged forbidden-file-name
done

# the same checks in the tree and history modes
newrepo
array 64 > "$r/notes.txt"
git -C "$r" add -A && git -C "$r" commit -q -m probe
expect_blocked "the tree mode finds a committed keypair array" --tree solana-keypair-array
expect_blocked "the history mode finds a committed keypair array" --history solana-keypair-array

# git ignores keypair files anywhere (the real .gitignore of the repository)
newrepo
cp "$scripts/../.gitignore" "$r/.gitignore"
mkdir -p "$r/programs/x" "$r/deploy"
echo '{}' > "$r/programs/x/id.json"
echo '{}' > "$r/deploy/main-keypair.json"
echo '{}' > "$r/keypair.json"
git -C "$r" add -A
if [ -z "$(git -C "$r" status --porcelain | grep -E 'id\.json|keypair')" ]; then ok; else bad "the .gitignore must ignore keypair files in every directory"; git -C "$r" status --porcelain >&2; fi

echo "secret scan: $passed checks passed"
[ "$failed" -eq 0 ]

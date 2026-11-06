#!/bin/sh
# Static consistency checks of Cargo.lock and the manifests, with no tools beyond sh, awk and find:
#   - every package that has a source comes from the crates.io registry (no git, no other registry, no path outside the repo)
#   - every such package carries a 64-digit hexadecimal checksum
#   - no package (name, version) appears twice
#   - the packages without a source are exactly the workspace crates (their names start with `vesting_`)
#   - no [patch] or [replace] section in any Cargo.toml, no .cargo/config file that could redirect a source
# The checksums themselves are proven by cargo, against the registry: scripts/verify-lock.sh.
# usage: check-lock.sh [DIR]    (default: the repository root)
set -eu
dir=${1:-$(dirname "$0")/..}
lock="$dir/Cargo.lock"
[ -f "$lock" ] || { echo "check-lock: no Cargo.lock in $dir" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
found=0
report() { printf 'check-lock: %s\n' "$1" >&2; found=1; }

awk '
  function flush() {
    if (name == "") return
    total++
    key = name " " version
    if (key in seen) printf "duplicate package %s\n", key
    seen[key] = 1
    if (source == "") {
      local++
      if (name !~ /^vesting_/) printf "package %s %s has no source and is not a workspace crate\n", name, version
    } else {
      if (source != "registry+https://github.com/rust-lang/crates.io-index") printf "package %s %s comes from %s, not from crates.io\n", name, version, source
      else if (checksum !~ /^[0-9a-f]{64}$/) printf "package %s %s has no valid checksum\n", name, version
      else registry++
    }
    name = ""; version = ""; source = ""; checksum = ""
  }
  /^\[\[package\]\]/ { flush(); name = "-"; next }
  /^name = / { gsub(/^name = "|"$/, ""); name = $0; next }
  /^version = "/ && name != "" && version == "" { gsub(/^version = "|"$/, ""); version = $0; next }
  /^source = / { gsub(/^source = "|"$/, ""); source = $0; next }
  /^checksum = / { gsub(/^checksum = "|"$/, ""); checksum = $0; next }
  END { flush(); printf "%d packages: %d from crates.io with a checksum, %d workspace crates\n", total, registry, local > "/dev/stderr" }
' "$lock" > "$tmp/problems" 2> "$tmp/summary" || true
while IFS= read -r line; do report "$line"; done < "$tmp/problems"
cat "$tmp/summary"

# the lock file format itself
grep -q '^version = [0-9]' "$lock" || report "Cargo.lock has no format version"

for manifest in $(find "$dir" -name Cargo.toml -not -path '*/target/*' -not -path '*/.git/*'); do
  if grep -E -q '^\[(patch|replace)' "$manifest"; then report "$manifest has a [patch] or [replace] section"; fi
done
for config in $(find "$dir" \( -name config -o -name config.toml \) -path '*/.cargo/*' -not -path '*/target/*' -not -path '*/.git/*'); do
  report "$config could redirect a dependency source"
done

[ "$found" -eq 0 ] || exit 1
echo "check-lock: clean"

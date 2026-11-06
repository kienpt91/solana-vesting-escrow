# The lock file: how it was made and how to prove it is consistent

`Cargo.lock` is committed, every cargo command in this repository runs with `--locked`, and **every crate in it was published
before 2026-10-29T00:00:00Z** (the cutoff of this repository: at least seven days before the work). The newest crate
versions that cargo's resolver picks are often younger than that, so the lock file was made in two steps and partly by hand.
This page records exactly what was done and how to check it. It is the one place where the repository's rules about
dependency age forced a manual edit of a generated file.

## How the lock file was made

1. `scripts/rs.sh --net 'cargo generate-lockfile'` once, with the direct dependencies pinned exactly in the manifests.
2. The age check (`scripts/check-crates-age.mjs`) listed 22 locked crates published after the cutoff.
3. Fourteen of them were stepped back one crate at a time with
   `scripts/rs.sh --net 'cargo update -p NAME@VERSION --precise OLDER'` (the wrapper accepts nothing else that updates the lock):

   | Crate | Resolved version | Stepped back to |
   | --- | --- | --- |
   | `async-compression` | 0.4.52 | 0.4.51 |
   | `cc` | 1.11.0 | 1.10.0 |
   | `compression-codecs` | 0.4.47 | 0.4.46 |
   | `crc32fast` | 1.5.4 | 1.5.3 |
   | `ctutils` | 0.4.8 | 0.4.6 |
   | `encoding_rs` | 0.8.44 | 0.8.42 |
   | `log` | 0.4.35 | 0.4.34 |
   | `mime` | 0.3.18 | 0.3.17 |
   | `test-case` | 3.4.1 | 3.4.0 |
   | `test-case-core` | 3.4.1 | 3.4.0 |
   | `test-case-macros` | 3.4.1 | 3.4.0 |
   | `toml_edit` | 0.25.18 | 0.25.17 |
   | `utf8_iter` | 1.1.0 | 1.0.4 |
   | `winnow` | 1.0.5 | 1.0.4 |

   (`utf_types 1.0.0`, which only `utf8_iter 1.1.0` pulled in, left the lock file with that step.)
4. Seven crates could not be stepped back with cargo: `wasm-bindgen`, `wasm-bindgen-macro`, `wasm-bindgen-macro-support`,
   `wasm-bindgen-shared`, `js-sys`, `web-sys` and `wasm-bindgen-futures` require each other with exact (`=`) version
   requirements, so cargo refuses to downgrade any one of them (`failed to select a version ... required by package ...`).
   Their `version` and `checksum` lines in `Cargo.lock` were **edited by hand** to one release family published on 2026-09-25.
   The checksums were copied from the crates.io API (`https://crates.io/api/v1/crates/NAME/VERSION`, field `checksum`):

   | Crate | Version | SHA-256 of the `.crate` file |
   | --- | --- | --- |
   | `wasm-bindgen` | 0.2.129 | `9bb54f33acc68fd454578d9820b0bde1a1a3d17aa17bb7b6595806d02886d409` |
   | `wasm-bindgen-macro` | 0.2.129 | `2e29d0c35b16e224a7eeb5cd2d25e3e1968fbd65604117b44d3b789d00ee8535` |
   | `wasm-bindgen-macro-support` | 0.2.129 | `6f501a8bc3719dba86ef8ae4728879c08001bea749eb1333ac5b91e040e2a6b7` |
   | `wasm-bindgen-shared` | 0.2.129 | `23f0c9c52aa7cd7d77769a4cfe2a9adb1b331f489a41d912ce14513d5ab995c6` |
   | `js-sys` | 0.3.106 | `7883d941dae510fb2d978fc3fe018c71c9e2892fd38854de3e8b92c2e5ad9cc5` |
   | `web-sys` | 0.3.106 | `88261b9deccee56594c11a3460c462c41f58d148598fe70ad77070126a68aba4` |
   | `wasm-bindgen-futures` | 0.4.79 | `3cbab34de2d982e9b48e18d216d04c4a6f641066ff19ffb699980f591ee3610e` |

   The `dependencies` lists of those entries were not touched: the two families have the same dependency lists.
5. No blanket `cargo update` was ever run; the wrapper refuses it.

`cargo tree` (host target) shows that `wasm-bindgen`, `wasm-bindgen-macro`, `wasm-bindgen-macro-support` and
`wasm-bindgen-shared` are part of the native build: `solana-program` 1.18.26 depends on `wasm-bindgen` for every target except
`solana`, so these four are compiled, and the macro crate runs, in every test build. `js-sys`, `web-sys` and
`wasm-bindgen-futures` are in the lock file only because cargo resolves every target; on the host `cargo tree` prints
"nothing to print" for them, they are used by `wasm32` dependencies only and are not compiled here.

## How to prove it is consistent

```sh
sh scripts/check-lock.sh          # static: sources, checksums present, no duplicates, no [patch], no .cargo/config
sh scripts/test-lock.sh           # the shell tests of that script, on hand-made lock files
sh scripts/verify-lock.sh         # everything below, needs docker and network (about 15 minutes)
```

`scripts/verify-lock.sh` runs, in this order:

1. `check-lock.sh` (static checks).
2. `cargo fetch --locked` into a **throwaway cargo volume**. Every crate is downloaded again, and cargo compares the SHA-256
   of each download with the `checksum` in `Cargo.lock` and with the registry index; a wrong or hand-edited checksum fails
   here. This is what proves the seven hand-edited entries: a persistent cargo volume would not, because it already holds
   the downloads.
3. `cargo metadata --locked --offline` on that volume: the lock file resolves the whole workspace without any change
   (`--locked` fails if cargo would have to rewrite it).
4. `node scripts/check-crates-age.mjs . 2026-10-29T00:00:00Z`: no locked crate is newer than the cutoff.

CI runs the same through the `lockfile` job. A pull request that changes `Cargo.lock` should be read with this page open:
the diff of a lock file is only trustworthy together with those four steps.

## What this does not prove

* It does not say the crates are free of vulnerabilities. The lock file holds known-advisory crates through the Solana 1.18
  family (see the README, "Dependencies"); no advisory database was queried (`cargo audit` and `cargo deny` are not in the
  image and the policy allows no further network).
* It does not prove that the registry itself is honest: cargo trusts the index and the checksums it publishes.
* The age check trusts the `created_at` field of the crates.io API.

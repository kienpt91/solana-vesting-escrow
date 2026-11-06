# ADR 0006: a native program with a hand-written validation layer, tested in process

Status: accepted.

## Context

Constraints of this repository: all tooling runs in containers, no Solana CLI toolchain may be downloaded, every crate must be
at least seven days old, and the machine has no validator.

1. *Anchor.* Anchor's CLI, its build tooling and a large part of its macro stack are not available under these rules, and the
   point of this repository is to show account design and validation by hand. The program is therefore written against
   `solana-program` with a small validation layer (`validation.rs`, `token.rs`). Porting to Anchor is covered in
   `docs/adapting.md`.
2. *SBF build.* The pinned Rust 1.95 image has no SBF target (`rustc --print target-list` lists only `bpfel-unknown-none`
   and `bpfeb-unknown-none`, which are not Solana's instruction set), and the compiler that can target SBF ships with the
   Solana toolchain, which may not be downloaded here. The program is built for the host only; this is stated wherever test
   results are.
3. *Harness.* `solana-program-test` runs an in-process bank with the real runtime rules. It can run a program as a native
   builtin, which is what is used for the vesting program. It also bundles the real SBF binaries of the SPL Token,
   Token-2022 and associated token account programs, which the tests use unchanged: the token side of every test is real
   bytecode in the real BPF virtual machine.
4. *Version.* `solana-program-test` 1.18.26 (with `solana-program` 1.18.26, `spl-token` 4.0.0). Newer families were tried:
   2.0.25 needs `protoc` to build (the build failed on it); 2.1.x to 3.1.x resolve with a vendored OpenSSL (`openssl-src`), whose
   build needs perl modules the pinned image does not ship (the 3.1.12 build failed on it; without the vendored build it needs
   `pkg-config` and the OpenSSL headers, which are not in the image either); 4.x was not tried. None of those tools may be
   installed into the pinned image. LiteSVM was considered and rejected: it runs SBF ELF files and its native hook takes the
   runtime's internal builtin function type. 1.18.26 builds with nothing but the image. The cost is an old ecosystem (see
   the README, "Dependencies") and no 2.x-only features.

## Decision

Native program, hand-written validation, `solana-program-test` 1.18.26 as the in-process bank, SPL programs as bundled SBF.

## Consequences

* The tests prove the program's logic and its account validation against the real runtime rules (ownership, signers,
  writability, privilege checks on CPIs, sysvars). They do not prove compute-unit use, stack and heap limits, or the SBF
  build of the vesting program. See the README for the full statement.
* Moving to a newer Solana family is a dependency change plus an image or tool change; it does not touch the program logic.

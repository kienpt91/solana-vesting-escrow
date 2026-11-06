# SBF readiness: what was done, what is checked statically, what is still unverified

**No SBF build of this program has ever been made.** The pinned Rust image has no SBF target and the Solana toolchain that
provides one may not be downloaded under this repository's rules (README, "What the tests execute"). The vesting program is
tested as a native build inside an in-process bank. This page lists what was prepared for an SBF build without the toolchain,
which of those preparations are checked mechanically, and what has to be checked on a real build before anyone relies on the
program. Statements about the Solana toolchain below are recollections, marked as such; none was verified here.

## Done without the toolchain

| Item | State | Check |
| --- | --- | --- |
| `crate-type = ["cdylib", "lib"]` in `programs/vesting_escrow/Cargo.toml` (I recall that `cargo build-sbf` links the program from a `cdylib`) | done | `architecture.rs::the_crate_type_lets_cargo_build_sbf_link_the_program`; the host build still produces a native `.so` with this setting (of no use on a cluster) |
| The entrypoint is behind a feature (`no-entrypoint`) so that other crates and the tests can link the library | done | `cargo build -p vesting_escrow --release` and `--features no-entrypoint` are both in CI |
| No heap allocation in the handlers, the state codec, the validation layer and the token helpers (SBF has a 32 KiB bump heap that is never freed) | checked statically | `architecture.rs::the_account_handling_code_does_not_allocate` (this covers this repository's code only: `solana-program` allocates while deserializing the accounts) |
| No stack array above 128 elements | checked statically | `architecture.rs::no_stack_array_is_larger_than_a_small_bound` |
| Sizes of the values held by the largest handler stay small (schedule record, parameters, instruction, two token accounts, mint: 928 bytes together in the host layout) | a heuristic, not a measurement | `programs/vesting_escrow/tests/layout.rs::the_values_the_handlers_keep_on_the_stack_stay_small` |
| CPI signer seeds are built in one place from the stored state; the only CPIs are the two vault transfers/close (signed by seeds) and the funding transfer (signed by the creator) | checked statically | `architecture.rs::every_cpi_signs_with_seeds_built_from_the_stored_state` |
| No `unsafe` outside the generated entrypoint | checked statically | `architecture.rs::unsafe_code_exists_only_in_the_entrypoint` |
| Direct dependencies are the ones of the Solana 1.18 era that `cargo build-sbf` of that era built | by choice | not checked |

## Expected blockers for `cargo build-sbf` (recollection, unverified)

1. **Lock file and `rust-version`.** `Cargo.lock` is lock-file format 4 and the manifests say `rust-version = "1.95"`. The
   `cargo` and `rustc` that ship with a Solana 1.18-era toolchain (the "platform tools") are, as I recall, several years older
   than 1.95 and would refuse both. A build there needs its own lock file (let that toolchain's cargo generate it, which
   gives up this repository's crate-age guarantee for that lock file: run `scripts/check-crates-age.mjs` on it) and a lowered
   `rust-version` for the program tree. Whether the program source compiles on that older compiler was not tried.
2. **A separate workspace member set.** Build only the program (`cargo build-sbf --manifest-path programs/vesting_escrow/Cargo.toml`)
   so that the test crates (which pull in the whole Solana runtime) are not part of the SBF build.
3. **Placeholder program id.** `lib.rs` declares ASCII text as the id; a deployment needs a real keypair kept outside the repository.

## To verify on a real SBF build, in this order

1. **It builds.** Read the compiler output for stack warnings ("Stack offset of N exceeded max offset of 4096"): that message,
   not the heuristic above, is the evidence on frame size. `initialize_schedule` is the function to look at first.
2. **The tests pass on the bytecode.** `Env::new` in `crates/vesting_tests/src/lib.rs` registers the program with
   `processor!(...)`; pass `None` instead and set `SBF_OUT_DIR` to the directory of the built `.so` and `solana-program-test`
   loads the bytecode. The attack suite then runs unchanged against the real VM. Expect to adapt one test: the one that asserts the
   program is registered as a native builtin (`happy_paths.rs::the_token_programs_are_sbf_binaries_and_the_vesting_program_is_a_native_builtin`).
3. **Compute units per instruction.** Add an assertion per instruction against a budget (`sol_log_compute_units` or the
   transaction's consumed units). The cost to look at: `Pubkey::find_program_address` runs on every instruction (up to 255
   iterations of about 1,500 units each in the worst case, by the Solana documentation, which was not re-checked); the program
   derives the schedule address again on every use instead of trusting the stored bump alone (ADR 0001).
4. **Account closing on the real runtime.** `close_program_account` zeroes the data, moves the lamports, assigns the account to the
   system program and reallocs it to length 0, in that order. The native bank accepts it and the test
   `a_closed_schedule_cannot_be_used_or_revived` shows the result, but the data-length change after the owner change goes through the
   SBF input buffer on a cluster, which was not exercised. This is the order that widely used programs use; that is a recollection,
   not a check.
5. **Account data after a CPI.** `initialize_schedule` reads the vault's balance after the funding CPI (`VaultFundingMismatch`).
   That relies on the runtime writing the callee's changes back into the caller's `AccountInfo`. In the native test this is done by
   `solana-program-test`'s stub of `invoke`, not by the BPF syscall. No `RefMut` borrow of account data or lamports is held across a CPI
   in the code (each `try_borrow_mut_data` guard ends within its statement), which is what makes the SBF path safe, but only a run on
   the real VM shows it. Duplicate mutable accounts, the aliasing hazard of CPIs, are refused up front by `validation::distinct`.
6. **Deployment hygiene.** Upgrade authority, program data, a verifiable build: `docs/audit-checklist.md`, section 7.

Until 1 to 5 are done, claims about compute use, stack use and the syscall path are claims about code review, not about tests.

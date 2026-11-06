# solana-vesting-escrow

A token vesting escrow for Solana, written from scratch in Rust: it locks SPL tokens in a vault that only a program derived
address can open, and releases them on a schedule (a cliff, then linear), with revocation by an authority, a two-step
beneficiary change and a safe close. It comes with a test suite that attacks it.

It is a portfolio project, built to show how I design accounts, PDAs, CPIs and on-chain state, how I write tests that try to
break a contract, and how I review one. It is not a tutorial program: the interesting part is the list of things it refuses
(`docs/threat-model.md`, the attack table below) and the checklist it was reviewed against (`docs/audit-checklist.md`).

> **Honest status.** Not audited. Never deployed to devnet, testnet or mainnet. No RPC calls, no keys, no network at test time.
> The tests run **in process**: the vesting program runs as a **native build** inside the test binary, not as SBF bytecode
> (compute units, stack and heap limits are therefore not tested). See [What the tests execute](#what-the-tests-execute)
> and [What the tests do not prove](#what-the-tests-do-not-prove). The CI and release workflows have not been run on GitHub:
> the repository has not been pushed.

## What it does

| Instruction | Who signs | Effect |
| --- | --- | --- |
| `initialize_schedule` | creator | creates the schedule account (a PDA), checks the vault (the associated token account of the schedule address) and moves `total` tokens into it |
| `claim` | beneficiary | pays `vested(now) - claimed` to the beneficiary's token account |
| `revoke` | authority (revocable schedules only) | the vested, unclaimed part goes to the beneficiary, the unvested remainder returns to the authority |
| `close` | authority | after everything was paid: sweeps stray tokens, closes the vault and the schedule account, returns the rent |
| `propose_beneficiary` / `accept_beneficiary` | current beneficiary / proposed key | two-step change of the beneficiary |

Rules worth knowing (all tested): time comes from the clock sysvar only, never from instruction data; nothing vests before
the cliff and everything vests at the end; `vested = floor(total * elapsed / duration)` so rounding never overpays
(ADR 0003); schedules are at most ten years long and must end in the future; a start in the past is accepted; the canonical
bump of the schedule address is found by the program, stored, and compared on every use; the classic SPL Token program is
the only token program (Token-2022 is refused, ADR 0005); extra accounts are refused; closing is only possible once
`claimed + returned == total`.

Account layouts, seeds and the account list of every instruction: `docs/accounts.md`.

## Quickstart

Only Docker is needed. Every cargo command runs in the official `rust` image pinned by digest
(`rust@sha256:d7482085ff5b415f84dba5647ae71606650bdef00db7aeb69f4b3d170c3e4082`: Rust 1.95.0, image built 2026-05-20), as a
non-root user, with all capabilities dropped, 4 CPUs and 8 GiB, the cargo registry, the rustup home and the target directory in
named volumes, and **network none** except for the two commands that must download something:

```sh
scripts/rs.sh --net 'rustup component add clippy rustfmt llvm-tools-preview'   # once: the image ships no clippy, rustfmt or llvm-tools
scripts/rs.sh --net 'cargo fetch --locked'                                      # the locked crates
scripts/rs.sh 'cargo fmt --all -- --check'
scripts/rs.sh 'cargo clippy --workspace --all-targets --locked --offline -- -D warnings'
scripts/rs.sh 'sh scripts/test-layers.sh'                                       # all tests, with a count per layer
scripts/rs.sh 'cargo build -p vesting_escrow --release --locked --offline'      # host build of the program
scripts/rs.sh 'sh scripts/demo.sh'                                              # curve, lifecycle, attack table
scripts/rs.sh 'sh scripts/coverage.sh'                                          # line coverage per crate
sh scripts/secret-scan.sh --tree && sh scripts/hygiene.sh --tree               # no secrets, no tool names or personal paths
node scripts/check-crates-age.mjs . 7                                           # every locked crate is at least 7 days old
```

The first build compiles about 600 crates (several minutes, about 9 GiB of target volume with the coverage build). One heavy job
at a time: the wrapper is meant for a shared machine. `rustup component add` is the only network use besides `cargo fetch`; the
`--net` flag of the wrapper refuses every other command.

## Architecture

```
crates/vesting_core        pure schedule logic: std only, no dependencies, #![forbid(unsafe_code)]
        ^                  validation, vested_at (floor), claimable, revocation split, checked u64/u128 maths
        |
programs/vesting_escrow    the on-chain program: state (hand-written layout), instruction data, PDA derivation,
        ^                  account-validation layer, token CPIs (classic SPL Token), error table; no maths of its own
        |
crates/vesting_client      SDK: address derivation, instruction builders, account decoding, status
        ^
        |
crates/vesting_tests       in-process harness (solana-program-test) and the suites that attack the program
```

* `vesting_core` knows nothing about Solana. `crates/vesting_tests/tests/architecture.rs` fails the build if it gains a
  dependency or a Solana name, if the program contains its own schedule arithmetic (`u128`, `checked_mul`, ...; clippy's
  `arithmetic_side_effects` is also denied there), if time is read anywhere but one `Clock::get()`, or if `unsafe` appears
  outside the generated entrypoint.
* The program is a native Solana program with a small hand-written validation layer (`validation.rs`, `token.rs`), not an
  Anchor program: Anchor's tooling is not available under this repository's rules (ADR 0006). `docs/adapting.md` describes the
  port.
* Why PDAs, why two-step beneficiary change, why rounding favours the vault, why revocation pays both sides, why only the classic
  token program, why a native program with in-process tests: `docs/adr/`.

## The attacks

`crates/vesting_tests/tests/attacks.rs` sends hostile transactions at a funded schedule. Every test names the attack, expects
exactly one error at exactly one instruction and then checks that no watched account changed. Run
`scripts/rs.sh 'sh scripts/demo.sh'` to print all 66 hostile transactions with the error expected and observed (the output of
a real run is in `docs/demo-output.txt`; the addresses in its second part differ from run to run, because the keypairs are generated
at run time).

| Attack | What the hostile transaction does | Rejected with | Test (`attacks.rs`) |
| --- | --- | --- | --- |
| Wrong signer | a stranger signs `claim` as the beneficiary | `NotBeneficiary` | `wrong_signer_claim_by_a_stranger` |
| Redirected payout | the beneficiary names someone else's token account; the authority redirects the unvested remainder | `WrongTokenAccountOwner` | `wrong_signer_beneficiary_cannot_redirect_to_another_owners_account`, `revoke_payout_cannot_be_redirected` |
| Missing signer | the signer flag is cleared and the signature left out (every signed instruction) | `MissingRequiredSignature` | `missing_signer_on_every_signed_instruction`, `missing_signer_on_initialize` |
| Wrong owner | schedule account owned by another program; vault owned by the system program; mint owned by the system program | `WrongAccountOwner` | `wrong_owner_schedule_account_owned_by_another_program`, `wrong_owner_vault_owned_by_the_system_program`, `wrong_owner_mint_owned_by_the_system_program` |
| Wrong mint | another mint account on `claim`/`revoke`; a funding or destination token account of another mint | `WrongMint`, `WrongTokenAccountMint` | `wrong_mint_on_claim_and_revoke`, `wrong_mint_source_token_account_of_another_mint`, `wrong_mint_destination_token_account_of_another_mint` |
| Substituted vault | another token account in the vault slot; a rogue account owned by the schedule address; another schedule's vault | `WrongVault` | `substituted_vault_a_token_account_of_the_attacker`, `substituted_vault_a_rogue_token_account_owned_by_the_schedule_address`, `substituted_vault_of_another_schedule` |
| Fake token program | a made-up id, the system program or a forged account in the token program slot | `WrongTokenProgram` | `fake_token_program_on_every_instruction` |
| Token-2022 | the Token-2022 program id, or a real Token-2022 mint, with the classic program | `UnsupportedTokenProgram` | `token_2022_is_refused_as_program_and_as_mint` |
| Fake system program | a made-up id, or the token program, in the system program slot | `WrongSystemProgram` | `fake_system_program_on_initialize` |
| Wrong PDA / bump | a valid address with a non-canonical bump; another creator's or id's address; a forged program-owned copy at a non-canonical address; a stored bump that is not canonical | `WrongScheduleAddress` | `wrong_pda_a_non_canonical_bump_address_at_initialize`, `wrong_pda_an_address_of_another_creator_or_id`, `wrong_pda_forged_schedule_at_a_non_canonical_address`, `wrong_bump_stored_in_the_schedule_does_not_match_the_canonical_bump` |
| Reinitialisation | `initialize_schedule` again on a live schedule, or over a program-owned zeroed account; use of a zeroed, truncated or padded account | `AccountAlreadyInitialized`, `InvalidAccountData` | `reinitialisation_of_a_live_schedule`, `reinitialisation_of_a_program_owned_account_with_zeroed_data`, `truncated_and_padded_schedule_data_is_refused` |
| Revival | use of a closed schedule, also after lamports were sent to its address | `WrongAccountOwner` | `a_closed_schedule_cannot_be_used_or_revived` |
| Double claim | the same `claim` twice in one transaction (the whole transaction is rolled back) and in two | `NothingToClaim` | `double_claim_in_one_transaction` |
| Claim before the cliff | one second early; before the start | `CliffNotReached` | `claim_before_the_cliff`, `claim_before_the_start` |
| Claim after revocation | the beneficiary claims after the authority revoked | `ScheduleRevoked` | `claim_after_a_revocation` |
| Revoke by non-authority | a stranger, the beneficiary, or the creator (when another key is the authority) | `NotAuthority` | `revoke_by_non_authority` |
| Revoke non-revocable | the authority revokes a non-revocable schedule | `NotRevocable` | `revoke_a_schedule_that_is_not_revocable` |
| Revoke twice / after fully vested | a second revocation; a revocation when everything vested | `ScheduleRevoked`, `FullyVested` | `revoke_twice_and_revoke_after_everything_vested` |
| Beneficiary swap by a stranger | a stranger, the authority or the creator proposes; a stranger accepts someone else's proposal; accept with nothing pending; the proposed key claims before accepting | `NotBeneficiary`, `NotPendingBeneficiary`, `NoPendingBeneficiary` | `beneficiary_swap_by_a_stranger`, `beneficiary_swap_acceptance_by_the_wrong_key`, `beneficiary_swap_to_invalid_keys` |
| Duplicate accounts | the vault as funding account or destination, the schedule as the vault, the mint as the funding account, on every instruction | `DuplicateAccount` | `duplicate_accounts_on_initialize`, `duplicate_accounts_on_claim_revoke_and_close` |
| Overflow amounts | a total of `u64::MAX` the creator does not hold; one token too many; `i64::MIN` start; `i64::MAX` end | `InsufficientFunds`, `NegativeTimestamp`, `DurationTooLong` | `overflow_amounts_on_initialize` |
| Corrupt accounting | claimed amount above the total in the stored state | `CorruptSchedule` | `corrupt_accounting_in_the_stored_schedule_is_refused` |
| Closing with a remaining balance | halfway; after a partial claim; vested but unclaimed (the authority cannot take the beneficiary's tokens) | `NotSettled` | `closing_with_a_remaining_balance` |
| Close by non-authority | a stranger or the beneficiary closes a settled schedule | `NotAuthority` | `closing_by_a_non_authority` |
| Clock manipulation | a timestamp appended to the data; data that is only a timestamp; a forged clock account appended or in another slot; the clock sysvar passed as an account | `InvalidInstructionData`, `UnexpectedAccounts`, `WrongTokenProgram` | `clock_manipulation_through_instruction_data`, `clock_manipulation_through_a_forged_clock_account` |
| Malformed input | empty or unknown data, trailing bytes, a flag of 2, short data, extra or missing accounts | `InvalidInstructionData`, `UnexpectedAccounts`, `NotEnoughAccountKeys` | `malformed_instruction_data_and_account_lists` |
| Read-only accounts | `claim` with the schedule, vault or destination read-only; `close` with the rent receiver read-only | `AccountNotWritable` | `read_only_accounts_that_must_be_written` |
| Token account state | claim from a frozen vault, into a frozen destination, from a frozen funding account; a vault with a delegate or a close authority; an uninitialised vault | `TokenAccountFrozen`, `UnsafeVaultAuthority`, `UninitializedAccount` | `frozen_accounts`, `a_vault_with_a_delegate_or_a_close_authority_is_refused`, `an_uninitialised_vault_is_refused` |
| Direct theft | the attacker signs a token transfer or `set_authority` on the vault; names the schedule address as an unsigned authority | token program `OwnerMismatch`, `MissingRequiredSignature` | `direct_withdrawal_from_the_vault_by_an_outsider` |
| Lamport drain | a system transfer or assign that names the schedule account as the source | `MissingRequiredSignature` | `lamports_cannot_be_drained_from_the_schedule_account` |
| Invalid parties | the default key as beneficiary or authority | `InvalidParty` | `initialize_with_the_default_key_as_beneficiary_or_authority` |
| Foreign funding | the creator names another owner's token account as the source | `WrongTokenAccountOwner` | `funding_account_of_somebody_else` |

Not attacks but guarded by tests of the same suite: a donation of lamports to the schedule address before creation does not
block it, and a donation of tokens to the vault does not block closing (`happy_paths.rs`).

## What the tests execute

`cargo test` here runs, in one process per test binary:

* **The vesting program as a native build.** It is compiled for the host (Linux, aarch64 or x86_64) into the test binary and
  registered with `solana-program-test` as a builtin (`processor!(vesting_escrow::processor::process_instruction)`). It is
  **not** SBF bytecode.
* **The SPL Token, Token-2022 and associated token account programs as real SBF binaries.** `solana-program-test` 1.18.26 bundles
  them (`spl_token-3.5.0.so`, `spl_token_2022-1.0.0.so`, `spl_associated_token_account-1.1.1.so`) and runs them in the bank's BPF
  virtual machine, so every token CPI of the vesting program is a call into real bytecode.
* **A real bank.** Transactions go through `BanksClient`: signature verification, account locking, the runtime's rules on
  ownership, writability and lamports after each instruction, the clock, rent and other sysvars, and the runtime's CPI processing.
  Time is moved by writing the clock sysvar; there is no real clock in a test.
* **Pure logic and byte formats without any runtime** (`vesting_core` tests, `programs/vesting_escrow/tests/layout.rs`).

Why not SBF? The pinned Rust image has no SBF target (`rustc --print target-list` lists only `bpfel-unknown-none` and
`bpfeb-unknown-none`, which are a different instruction set) and the compiler that can target SBF ships with the Solana
toolchain, which may not be downloaded under this repository's rules. Investigating first, as the brief asked: (a) SBF build
inside the container: not possible with what is obtainable from aged crates and the official image; (b) in-process native tests
through a harness: possible and used. Why not LiteSVM or a newer `solana-program-test`? LiteSVM loads SBF ELF files only. Newer
`solana-program-test` versions need `protoc` (2.0.x) or a vendored OpenSSL build with perl modules (2.1 to 4.x) that the pinned
image does not have, and nothing may be installed into it. Version 1.18.26 builds with the image alone (ADR 0006).

## What the tests do not prove

* **Compute units.** A native build is not metered like SBF. How many units each instruction costs, and whether
  `find_program_address` on every instruction fits a realistic budget with several CPIs, is unmeasured.
* **Stack and heap limits.** SBF has 4 KiB stack frames and a 32 KiB heap; the host has megabytes. A function that is fine here
  can overflow there. The code keeps frames small (no large arrays, the largest value is a 165-byte token account) but that is
  a review claim, not a test result.
* **The syscall path.** In a test the program calls `Clock::get`, `Rent::get`, `find_program_address` and `invoke_signed`
  through `solana-program-test`'s stubs, which call into the runtime in Rust. The BPF syscall implementations (pointer
  translation, memory regions, account data realloc across CPI) are not exercised for the vesting program.
* **The SBF build itself**: whether the program compiles for the SBF target, its size, whether it loads on a cluster.
* **A real cluster**: fees, priority, versioned transactions, address lookup tables, durable nonces, clock drift, upgradeable
  loader behaviour.
* **Token-2022** and wrapped SOL: refused or untested, by design (threat model).
* **Absence of bugs.** 140 passing tests are 140 passing tests. See "Where to attack first".

## Tests and coverage

| Layer | Where | Tests |
| --- | --- | ---: |
| core: validation, worked examples | `crates/vesting_core/tests/schedule_examples.rs` | 20 |
| core: seeded property tests | `crates/vesting_core/tests/properties.rs` | 9 |
| program: layout, instruction data, errors, addresses | `programs/vesting_escrow/tests/layout.rs` | 13 |
| client: instruction builders | `crates/vesting_client/tests/builders.rs` | 6 |
| happy paths (in-process bank) | `crates/vesting_tests/tests/happy_paths.rs` | 20 |
| edge cases (in-process bank) | `crates/vesting_tests/tests/edge_cases.rs` | 12 |
| attacks (in-process bank, 66 hostile transactions) | `crates/vesting_tests/tests/attacks.rs` | 52 |
| architecture rules | `crates/vesting_tests/tests/architecture.rs` | 8 |
| **total** | | **140** |

The property tests use a seeded SplitMix64 generator (no external crate) and check: vested is monotonic and never above the
total; nothing vests before the cliff and everything at the end; rounding is exactly the floor and never overpays; claims at
arbitrary times sum to exactly the total; a revocation conserves every token; `u64` edges never overflow; and a count-based
oracle that shares no code with the division agrees on every small schedule.

Coverage (rustc source-based instrumentation, `scripts/coverage.sh`, all tests of the workspace; lines of library code, the
vesting program measured while it runs natively in the bank):

| Crate | Lines | Covered | Percent |
| --- | ---: | ---: | ---: |
| `vesting_core` (pure logic) | 103 | 103 | 100.0% |
| `vesting_escrow` (program) | 646 | 637 | 98.6% |
| `vesting_client` (SDK) | 126 | 126 | 100.0% |
| all counted code | 875 | 866 | 99.0% |

The nine uncovered lines are in `processor.rs` (seven) and `token.rs` (two): the rent-exemption check after creating the
schedule account and the vault-growth check after funding (both defensive: the runtime and the fee-free classic token program
cannot make them fire), and the `?` of CPI failures that earlier checks make unreachable. Caveats: this is line coverage, not branch
coverage; the generated entrypoint is not counted (it is compiled out of the tests); `llvm-cov` printed "4 functions have
mismatched data" (generic functions instantiated in several test binaries), so a few lines may be off. These are the numbers
of one run, `docs/demo-output.txt` is the output of another; neither is a gate in CI.

## Dependencies

Exact pins, `Cargo.lock` committed, `--locked` everywhere. **Every one of the 590 crates in the lock file was published
before 2026-10-29T00:00:00Z** (`node scripts/check-crates-age.mjs . 2026-10-29T00:00:00Z` -> `too new: 0` on 2026-11-06; CI
runs it with a window of seven days).

| Crate | Pin | Published | Used by | Why |
| --- | --- | --- | --- | --- |
| `vesting_core` | path | | all | the schedule maths. No dependencies |
| `solana-program` | `=1.18.26` | 2024-10-11 | program, client, tests | the on-chain API: `AccountInfo`, entrypoint, CPI, sysvars, `Pubkey` |
| `spl-token` (`no-entrypoint`) | `=4.0.0` | 2023-06-27 | program, client, tests | instruction builders and the layout of mint and token accounts for the token CPIs; its processor is not used |
| `solana-program-test` | `=1.18.26` | 2024-10-11 | tests | the in-process bank and the bundled SPL programs |
| `solana-sdk` | `=1.18.26` | 2024-10-11 | tests | `Keypair`, `Transaction`, `Account`, `system_instruction` for the harness |
| `tokio` (`macros`, `rt`) | `=1.51.6` | 2026-10-13 | tests | the async runtime that `BanksClient` needs |

No other direct dependency: no `borsh`, no `thiserror`, no `bytemuck`, no `anchor-*`. The program's own normal-dependency
tree is 151 crates (`solana-program` and what it pulls in); the test crate's tree is 517 crates because of
`solana-program-test` (it embeds the whole runtime). Both numbers are from `cargo tree -e normal`.

**Why the Solana 1.18 family.** See ADR 0006: newer program-test families need tools the pinned image lacks. The cost is an old
ecosystem: the lock file contains `curve25519-dalek 3.2.1` and `ed25519-dalek 1.0.1`, which I know to carry published
advisories (RUSTSEC-2024-0344, RUSTSEC-2022-0093; recalled from memory, **no advisory database was queried**, `cargo audit` and
`cargo deny` were not run: they are not in the image and the policy allows no further network). Those crates come through
`solana-program`/`solana-sdk` and `solana-program-test`; the vesting code does not call them directly.

**How the lock file was made.** `cargo generate-lockfile` once, then every crate newer than the cutoff was stepped back:
fourteen with `cargo update -p NAME@VERSION --precise OLDER` (a single crate each time: `async-compression`, `cc`,
`compression-codecs`, `crc32fast`, `ctutils`, `encoding_rs`, `log`, `mime`, `test-case`, `test-case-core`, `test-case-macros`,
`toml_edit`, `utf8_iter`, `winnow`), and the seven crates of the `wasm-bindgen` family (`wasm-bindgen`, `-macro`,
`-macro-support`, `-shared`, `-futures`, `js-sys`, `web-sys`, which pin each other with `=` and cannot be stepped back one by
one) by editing their versions and checksums in `Cargo.lock` by hand, after which `cargo metadata --locked` accepts the file.
No blanket `cargo update` was run. The wrapper refuses `cargo update` without `-p ... --precise`.

## Limitations

* Not audited; not deployed; the tests are in-process and the program is a native build (see above).
* One token program (classic SPL Token). Wrapped SOL is not special-cased.
* A mint's freeze authority can freeze the vault or a payee; payouts then fail with `TokenAccountFrozen` and a revocation waits
  for the beneficiary's account to be thawed (ADR 0004).
* There is no instruction to change the authority or to cancel a proposal (a new proposal replaces it).
* No on-chain metrics, no events, no upgrade-authority handling: that is a deployment concern (`docs/audit-checklist.md`, section 7).
* The program id in `programs/vesting_escrow/src/lib.rs` is a placeholder made of ASCII text; the program derives everything from
  the id the runtime passes in.
* `docs/adapting.md` lists the changes a real use would need (schedule shapes, Token-2022 subset, Anchor port, newer crates, SBF
  harness).

## Where to attack first

If you review this, in this order:

1. **The claim path and the vault accounting**: `processor.rs::claim` and `revoke`. The state is written before the token CPI;
   confirm no path pays without updating `claimed_amount` and none updates it without paying.
2. **`close` and the surplus sweep**: it sweeps the whole vault balance to the authority once `claimed + returned == total`.
   Look for a way to make that condition true while the beneficiary is still owed something.
3. **`load_schedule`**: owner, exact length, discriminator, canonical address and stored bump. Try account data the program
   itself could have produced in an earlier layout version.
4. **Token account validation** (`token.rs`): everything the program does not check on payee accounts (it allows delegates and
   close authorities there, on purpose) and on the mint (it allows a freeze authority).
5. **The pure maths** (`schedule.rs`) against your own oracle, at `u64::MAX` and at the ten-year bound.
6. **What the native tests cannot see**: compute units of `find_program_address` on every instruction, stack frames, the SBF build.

## Layout

```
Cargo.toml, Cargo.lock, rust-toolchain.toml, rustfmt.toml, clippy.toml
crates/vesting_core/       pure logic, examples/curve.rs, tests/{schedule_examples,properties}.rs
programs/vesting_escrow/   src/{lib,state,instruction,pda,processor,validation,token,error,codec,entrypoint}.rs, tests/layout.rs
crates/vesting_client/     SDK, tests/builders.rs
crates/vesting_tests/      src/{lib,attack}.rs harness, tests/{happy_paths,edge_cases,attacks,architecture}.rs, examples/walkthrough.rs
docs/                      accounts.md, threat-model.md, audit-checklist.md, adapting.md, adr/, demo-output.txt
scripts/                   rs.sh (container wrapper), test-layers.sh, demo.sh, coverage.sh, secret-scan.sh, hygiene.sh,
                           check-commit-msg.sh, check-crates-age.mjs, release scripts
.github/                   workflows/{ci,release}.yml, dependabot.yml
SECURITY.md, CHANGELOG.md, RELEASING.md, LICENSE
```

## License

MIT, Copyright (c) 2026 Kien Phan. See `LICENSE`.

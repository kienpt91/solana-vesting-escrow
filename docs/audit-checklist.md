# Audit checklist

The checklist used to review a Solana program, and the format of a review deliverable. For a client review, copy this
file, keep the headings, replace the "vesting_escrow" column with the result for the reviewed program and add findings to
the table at the end. For this repository the right-hand column is filled in as a worked example: it says what was
checked and where the evidence is. It is a self-review by the author, not an independent audit; one independent read-only review of the repository
was done afterwards and its findings, with what was done about each, are in section 9.

Result values: **PASS** (checked, no issue), **FINDING** (see the findings table), **N/A** (the program has no such surface),
**NOT CHECKED** (say why).

## 0. Scope and setup

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 0.1 | Commit hash reviewed, build command, toolchain and dependency lock | the commit under review; `scripts/rs.sh`, pinned image digest, `Cargo.lock` committed |
| 0.2 | Program ids, upgrade authority, deployed hash (if deployed) | PASS (N/A deployment): never deployed; the program id in `lib.rs` is a placeholder |
| 0.3 | Instruction list with accounts, signers, writable flags | PASS: `docs/accounts.md` |
| 0.4 | Roles and trust assumptions written down | PASS: `docs/threat-model.md` |
| 0.5 | Tests run, with counts, and what they execute (native or SBF) | PASS: README, "What the tests execute" |

## 1. Signer and owner checks (every account, every instruction)

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 1.1 | Each authority account is a signer, checked through the signer flag, not through the key alone | PASS: `validation::signer` first in every handler; `missing_signer_on_every_signed_instruction` |
| 1.2 | Each authority key is compared with the key stored in state (not with a key from instruction data) | PASS: `key_is(.., &state.beneficiary / authority / pending_beneficiary)`; `wrong_signer_claim_by_a_stranger`, `revoke_by_non_authority` |
| 1.3 | Every program-owned account is checked for owner, length and discriminator before its data is trusted | PASS: `load_schedule`; `wrong_owner_*`, `truncated_and_padded_schedule_data_is_refused` |
| 1.4 | Every token account and mint is checked for owner (token program), mint, owner field, state (initialized, not frozen) | PASS: `token::{load_mint, load_account, check_account, check_vault}` |
| 1.5 | Writable flags are checked for every account the instruction modifies | PASS: `validation::writable`; `read_only_accounts_that_must_be_written` |
| 1.6 | Account list length is exact; no `remaining_accounts` are consumed silently | PASS: `validation::exact`; `malformed_instruction_data_and_account_lists` (remaining accounts: none exist) |
| 1.7 | The same account cannot occupy two roles that must differ | PASS: `validation::distinct`; `duplicate_accounts_*` |
| 1.8 | Signer privileges are not forwarded to a CPI target that does not need them | PASS: only the creator (for the funding transfer) and the schedule address (by seeds) sign CPIs |

## 2. PDA seeds and bumps

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 2.1 | Seeds include everything that makes the account unique and nothing that can change (no mutable owner in the seeds) | PASS: creator, mint, id; the beneficiary is not in the seeds (`docs/accounts.md`, ADR 0001) |
| 2.2 | Seeds are fixed-width or length-delimited so two different tuples cannot produce the same bytes | PASS: fixed prefix, two 32-byte keys, one u64 |
| 2.3 | The canonical bump is found by the program, stored, and compared on every use; no bump from instruction data | PASS: `pda::schedule_address`, `load_schedule`; `wrong_pda_*`, `wrong_bump_*` |
| 2.4 | A creator cannot be front-run into an occupied address | PASS: the creator is in the seeds; lamports sent first do not block creation (`lamports_sent_to_the_schedule_address_first_do_not_block_creation`) |
| 2.5 | Signer seeds passed to CPIs reproduce the address, including the bump | PASS: `SignerSeeds` is built from the stored fields; every claim, revoke and close test pays out through it |

## 3. CPI targets

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 3.1 | The program id of every CPI target is compared with a constant before the CPI is built | PASS: `token::check_program`, system program check in `initialize_schedule`; `fake_token_program_*`, `fake_system_program_*` |
| 3.2 | Token-2022 versus classic token program is decided explicitly | PASS: classic only, Token-2022 refused; `token_2022_is_refused_as_program_and_as_mint` |
| 3.3 | State is written before the CPI, or the CPI cannot re-enter | PASS: state first; the targets cannot call back (threat model, "Re-entrancy") |
| 3.4 | Balances after a CPI are verified where the program relies on them | PASS: vault growth checked after funding (`VaultFundingMismatch`) |
| 3.5 | Return data and error codes of a CPI are not swallowed | PASS: every CPI result is propagated with `?` |

## 4. Arithmetic

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 4.1 | All arithmetic is checked (or provably cannot overflow) and an overflow is an error, not a wrap or a saturate | PASS: clippy `arithmetic_side_effects` is denied in the program and the pure crate; checked ops in `vesting_core` |
| 4.2 | Rounding direction is chosen deliberately and favours the pool | PASS: floor division; ADR 0003; `crates/vesting_core/tests/properties.rs` |
| 4.3 | Edge values: zero, one, `u64::MAX`, `i64::MIN`/`MAX` timestamps | PASS: `edge_cases.rs`, `overflow_amounts_on_initialize`, `u64_edges_never_panic_and_stay_within_the_total` |
| 4.4 | Time comes from the clock sysvar only, never from instruction data or an account the caller supplies | PASS: `clock_manipulation_*`, `time_comes_from_the_clock_sysvar_only` |
| 4.5 | Casts between integer widths are checked | PASS: no narrowing casts of money; `u64::try_from` in the pure crate |

## 5. Account lifecycle: initialisation and closing

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 5.1 | Initialisation fails if the account is already initialised, including a program-owned zeroed account | PASS: `reinitialisation_*` |
| 5.2 | Initialisation cannot be blocked by pre-funding the address | PASS: `create_schedule_account` handles a non-zero balance |
| 5.3 | Accounts are rent exempt after creation and cannot be drained below it | PASS: checked after creation; only `close` removes lamports |
| 5.4 | Closing zeroes the data, moves all lamports, reassigns the owner to the system program and sets the length to zero | PASS: `close_program_account`; `close_after_completion_*`, `a_closed_schedule_cannot_be_used_or_revived` |
| 5.5 | A closed account cannot be revived within the same transaction or by a lamport donation | PASS: `a_closed_schedule_cannot_be_used_or_revived` (lamports sent in a later transaction), `a_closed_schedule_cannot_be_revived_in_the_same_transaction` (close, send lamports, claim, in one transaction) |
| 5.6 | Closing is only possible when nothing is owed, and only by the party entitled to the rent | PASS: `closing_with_a_remaining_balance`, `closing_by_a_non_authority` |
| 5.7 | Donated tokens or lamports cannot block a lifecycle step | PASS: `a_donation_to_the_vault_is_swept_to_the_authority_on_close` |

## 6. Token-specific surface

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 6.1 | Mint extensions (Token-2022): transfer fee, transfer hook, permanent delegate, confidential transfer, interest-bearing, non-transferable, default account state, pausable | PASS (refused): Token-2022 is refused as program and as mint |
| 6.2 | Delegates and close authorities on accounts the program relies on | PASS: refused on the vault, allowed on payees (`a_vault_with_a_delegate_or_a_close_authority_is_refused`, `a_destination_with_a_delegate_and_a_close_authority_can_still_be_paid`) |
| 6.3 | Frozen accounts and mint freeze authority | PASS: frozen accounts are refused with a precise error; the freeze authority is an accepted risk (threat model) |
| 6.4 | Wrong mint, wrong owner, wrong decimals | PASS: `TransferChecked` with the mint's decimals; `wrong_mint_*`, `mints_with_zero_and_nine_decimals_work` |
| 6.5 | Native (wrapped SOL) mint behaviour | NOT CHECKED: not special-cased and not tested; treat as unsupported |
| 6.6 | Associated token account derivation is verified, not assumed | PASS: `pda::vault_address`, `WrongVault` |

## 7. Upgrade authority and deployment

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 7.1 | Who holds the upgrade authority, is it a multisig, is there a timelock, can it be revoked | N/A: not deployed. A deployment note must state this |
| 7.2 | Program data account and verifiable build (same source, same hash) | NOT CHECKED: no SBF build exists under this repository's tooling policy (README); preparation and the open items are in `docs/sbf-checklist.md` |
| 7.3 | Program id declared in source matches the deployment | N/A: the id in `lib.rs` is a placeholder; the program derives everything from the id the runtime passes in |
| 7.4 | Fee payer and rent payer roles, and who receives refunds | PASS: the creator pays rent, the authority receives it back on close (`docs/accounts.md`) |

## 8. Reinitialisation, remaining accounts and other classics

| # | Item | vesting_escrow |
| ---: | --- | --- |
| 8.1 | Reinitialisation after close (resurrection) | PASS: closed schedule is a plain system account; an address may be reused for a fresh schedule only through `initialize_schedule` (`an_address_can_be_used_again_after_the_schedule_is_closed`) |
| 8.2 | `remaining_accounts` / extra accounts used for authorisation or CPI targets | PASS: extra accounts are refused (`UnexpectedAccounts`) |
| 8.3 | Type cosplay: one account type accepted as another | PASS: discriminator and exact length; the only program-owned account type is the schedule |
| 8.4 | Sysvar accounts passed by the caller instead of read from the syscall | PASS: none are accepted; `clock_manipulation_through_a_forged_clock_account` |
| 8.5 | Instruction data parsing: trailing bytes, unknown tags, flag values | PASS: `instruction_data_is_parsed_strictly`, `malformed_data_*` |
| 8.6 | Panics reachable from user input (`unwrap`, indexing, slicing) | PASS: `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` denied by clippy in the program and the pure crate |
| 8.7 | Compute budget and stack/heap on the target VM | NOT CHECKED: the tests execute the program natively (README). Static checks only: no allocation in the handlers, no large stack array, small value sizes (`docs/sbf-checklist.md`) |
| 8.8 | Logging that leaks or misleads | PASS: one fixed line per instruction, no user data |

## 9. Findings

One independent read-only review was done after the first version of this repository. It found no exploitable flaw in the
on-chain logic; its findings concerned the evidence, the tooling claims, SBF readiness and the documentation. Status as of the
commit that fixed them:

| ID | Severity | Where | Description | Status |
| --- | --- | --- | --- | --- |
| R1 | Low | `scripts/demo.sh` | The attack table dropped the first row of every attack test (libtest glues `test NAME ... ` in front of it), so "66 hostile transactions" was wrong (the real number is in `docs/demo-output.txt`), and the exit status of cargo was masked | Fixed: rows start on their own line and are numbered; `scripts/attack-table.sh` keeps cargo's status and refuses gaps, repeats, FAIL rows, fewer rows than tests and a missing summary; `scripts/test-demo.sh` tests it; `architecture.rs` ties the figure in README and SECURITY to the demo output |
| R2 | Low | `crates/vesting_tests/src/lib.rs` | A token program error code was printed with the name of the vesting error of the same number | Fixed: a failure records the program it came from and is named from that program's table (`harness.rs`); the direct-attack tests assert the token program's `OwnerMismatch` by program |
| R3 | Low | build | No `cdylib` crate type; lock file format 4 and `rust-version 1.95` against a Solana 1.18-era toolchain | Crate type fixed. Lock file and `rust-version`: documented, not changed, because the effect on an SBF build cannot be verified without the toolchain (`docs/sbf-checklist.md`) |
| R4 | Low | `scripts/rs.sh` | The `--net` allow list was a prefix match and could be chained | Fixed: only letters, digits, spaces and `. _ @ + -` are accepted, and the commands match exactly (`scripts/test-rs.sh`) |
| R5 | Low | `scripts/secret-scan.sh`, `.gitignore` | No rule for Solana keypair files, patterns anchored to the root | Fixed: keypair array and base58 rules, file names, unanchored ignore patterns (`scripts/test-secret-scan.sh`) |
| R6 | Info | docs | Statements stronger than the code: "checked on every load"; the stack-frame sentence; "same transaction" revival; "no findings" | Fixed: reworded (`docs/accounts.md`, README), a same-transaction test added, this table filled in. The pending proposal is now cleared on revocation |
| R7 | Info | `processor.rs::revoke` | A frozen beneficiary account blocked a revocation even when the beneficiary was owed nothing | Fixed: the account is checked only when it is paid (`edge_cases.rs`); the case where it is paid stays a documented trade-off (ADR 0004) |
| R8 | Info | `Cargo.lock` | Crates with known advisories (Solana 1.18 family); hand-edited `wasm-bindgen` entries | Accepted and disclosed; the hand edits are listed and verifiable (`docs/lockfile.md`, `scripts/verify-lock.sh`); no advisory database was queried |

Severity scale: **Critical** (loss of funds without special preconditions), **High** (loss of funds with preconditions or
permanent lock-up), **Medium** (denial of service, wrong accounting without loss), **Low** (hardening), **Info**.

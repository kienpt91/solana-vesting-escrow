# Security

## Status

This is a reference implementation. It has **not been audited**, it has **not been deployed** to devnet, testnet or mainnet,
and its tests run **in process** (the vesting program as a native build inside a test binary, the SPL programs as the SBF
binaries bundled with `solana-program-test`). Do not use it to hold value without an independent audit and a test run on
SBF bytecode. No statement in this repository is a claim about mainnet behaviour.

## Reporting a problem

Open a private security advisory on the repository's GitHub page, or write to the address in the git history
(`git log -1 --format=%ae`). Please include the instruction, the account list, the expected error and what happened.
There is no bounty and no deployment to protect; reports are still welcome.

## Threat model in one page

The full model, with actors, assets, accepted risks and every test behind every claim, is `docs/threat-model.md`.
The review checklist applied to this program is `docs/audit-checklist.md`. The claims, one test each:

| Claim | Test (crates/vesting_tests/tests unless noted) |
| --- | --- |
| Only the beneficiary can claim | `attacks.rs::wrong_signer_claim_by_a_stranger` |
| A required signature is a signature, not a flag | `attacks.rs::missing_signer_on_every_signed_instruction` |
| Only the authority can revoke or close | `attacks.rs::revoke_by_non_authority`, `attacks.rs::closing_by_a_non_authority` |
| A non-revocable schedule cannot be revoked | `attacks.rs::revoke_a_schedule_that_is_not_revocable` |
| Nobody can take over the beneficiary role | `attacks.rs::beneficiary_swap_by_a_stranger`, `attacks.rs::beneficiary_swap_acceptance_by_the_wrong_key` |
| Payouts cannot be redirected | `attacks.rs::revoke_payout_cannot_be_redirected`, `attacks.rs::wrong_signer_beneficiary_cannot_redirect_to_another_owners_account` |
| The vault cannot be swapped for another account | `attacks.rs::substituted_vault_a_rogue_token_account_owned_by_the_schedule_address` |
| Wrong mints, wrong owners and fake programs are refused | `attacks.rs::wrong_mint_on_claim_and_revoke`, `attacks.rs::wrong_owner_vault_owned_by_the_system_program`, `attacks.rs::fake_token_program_on_every_instruction`, `attacks.rs::fake_system_program_on_initialize` |
| Token-2022 is refused | `attacks.rs::token_2022_is_refused_as_program_and_as_mint` |
| The schedule address is canonical, and so is its stored bump | `attacks.rs::wrong_pda_a_non_canonical_bump_address_at_initialize`, `attacks.rs::wrong_pda_forged_schedule_at_a_non_canonical_address`, `attacks.rs::wrong_bump_stored_in_the_schedule_does_not_match_the_canonical_bump` |
| A schedule cannot be reinitialised or revived | `attacks.rs::reinitialisation_of_a_live_schedule`, `attacks.rs::a_closed_schedule_cannot_be_used_or_revived` |
| The same account cannot play two roles | `attacks.rs::duplicate_accounts_on_initialize`, `attacks.rs::duplicate_accounts_on_claim_revoke_and_close` |
| Time comes from the clock sysvar only | `attacks.rs::clock_manipulation_through_instruction_data`, `attacks.rs::clock_manipulation_through_a_forged_clock_account`, `architecture.rs::time_comes_from_the_clock_sysvar_only` |
| Nothing is claimable before the cliff, and a claim pays once | `attacks.rs::claim_before_the_cliff`, `attacks.rs::double_claim_in_one_transaction` |
| A schedule cannot be closed while tokens are owed | `attacks.rs::closing_with_a_remaining_balance` |
| Overflowing amounts and timestamps are refused | `attacks.rs::overflow_amounts_on_initialize`, `edge_cases.rs::invalid_schedule_parameters_are_refused_with_their_own_error` |
| Rounding never overpays; claims sum to the total | `crates/vesting_core/tests/properties.rs::rounding_never_overpays_and_is_the_exact_floor`, `crates/vesting_core/tests/properties.rs::claims_over_time_sum_to_exactly_the_total` |
| The pure crate is pure | `architecture.rs::the_pure_crate_has_no_dependencies_at_all` |
| Vault delegates and close authorities, frozen accounts are refused | `attacks.rs::a_vault_with_a_delegate_or_a_close_authority_is_refused`, `attacks.rs::frozen_accounts` |
| A donation of lamports or tokens cannot block creation or closing | `happy_paths.rs::lamports_sent_to_the_schedule_address_first_do_not_block_creation`, `happy_paths.rs::a_donation_to_the_vault_is_swept_to_the_authority_on_close` |

`scripts/demo.sh` prints every hostile transaction with the error expected and the error observed (`docs/demo-output.txt`).

## Not covered

Compute-unit use and stack and heap limits (the program is not run as SBF bytecode here), upgrade authority and deployment
practice (nothing is deployed), Token-2022 (refused), wrapped SOL (not special-cased), the behaviour of a real cluster, and
anything an independent audit would find. See the README for the limitations and for what a reviewer should attack first.

## Supply chain

Dependencies are pinned exactly, `Cargo.lock` is committed, every crate in it was published at least seven days before the
cutoff recorded in the README (`scripts/check-crates-age.mjs`), builds run in a container pinned by digest with network
none after the crates are fetched, and the secret scanner and the commit hygiene check run in the git hooks and in CI.
No private key is committed: the keypairs the tests use are generated in memory at run time.

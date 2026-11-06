# Threat model

Scope: the on-chain program in `programs/vesting_escrow` and the pure logic in `crates/vesting_core`. Every claim below
names the test that exercises it. Test names refer to files under `crates/vesting_tests/tests/` unless a path is given;
`attacks.rs` prints one `ATTACK|...` line per hostile transaction (see `docs/demo-output.txt`).

This program has not been audited and has not been deployed to any cluster. The tests run in-process (see the README
section "What the tests execute"). Nothing here is a statement about mainnet behaviour.

## Assets

1. **Vested tokens in the vault.** Tokens that the creator locked. The beneficiary is entitled to the vested part, the
   authority (if the schedule is revocable) to the unvested part.
2. **The schedule record.** Its numbers decide who may take what and when.
3. **Rent lamports** of the schedule account and the vault.

## Actors

| Actor | Trusted to | Not trusted to |
| --- | --- | --- |
| Creator | choose the schedule and fund it | change it later (there is no instruction for that) |
| Beneficiary | claim vested tokens; hand the schedule to another key | claim more, claim early, redirect the authority's share |
| Authority | revoke (if allowed), close a settled schedule | touch vested tokens, revoke a non-revocable schedule, redirect payouts |
| Anyone | pay fees, send tokens or lamports to any address | do anything else |
| Token program, system program, sysvars | behave as specified | (fixed ids; any other id is refused) |

The mint's freeze authority and mint authority are outside the model: a mint that can freeze accounts can freeze the vault
(`frozen_accounts` shows the program refusing to pay out of or into a frozen account instead of failing deep inside a CPI).

## Claims and the tests behind them

### Authorisation

| Claim | Test |
| --- | --- |
| Only the beneficiary claims; a stranger signing in its place is refused (`NotBeneficiary`) | `wrong_signer_claim_by_a_stranger` |
| Every instruction needs its signer's signature, not only its signer flag in the account list | `missing_signer_on_every_signed_instruction`, `missing_signer_on_initialize` |
| Only the authority revokes and closes (`NotAuthority`): not a stranger, not the beneficiary, not the creator when another key is the authority | `revoke_by_non_authority`, `closing_by_a_non_authority` |
| A non-revocable schedule cannot be revoked (`NotRevocable`) | `revoke_a_schedule_that_is_not_revocable` |
| Only the current beneficiary can propose a successor, and only the proposed key can accept | `beneficiary_swap_by_a_stranger`, `beneficiary_swap_acceptance_by_the_wrong_key`, `beneficiary_swap_to_invalid_keys` |
| A creator cannot fund a schedule from somebody else's token account | `funding_account_of_somebody_else` |

### Funds cannot be redirected or taken out of band

| Claim | Test |
| --- | --- |
| The vested part can only be paid to a token account of the beneficiary, the unvested part only to one of the authority | `wrong_signer_beneficiary_cannot_redirect_to_another_owners_account`, `revoke_payout_cannot_be_redirected` |
| Nobody but the schedule address can move tokens out of the vault; the token program refuses the attacker | `direct_withdrawal_from_the_vault_by_an_outsider` |
| Lamports cannot be taken from the program-owned schedule account by other instructions | `lamports_cannot_be_drained_from_the_schedule_account` |
| The authority cannot take tokens that have vested but were not yet claimed (`close` needs `claimed + returned == total`) | `closing_with_a_remaining_balance` |

### Account substitution

| Claim | Test |
| --- | --- |
| A different vault (another token account, a rogue account owned by the schedule address, another schedule's vault) is refused (`WrongVault`) | `substituted_vault_a_token_account_of_the_attacker`, `substituted_vault_a_rogue_token_account_owned_by_the_schedule_address`, `substituted_vault_of_another_schedule` |
| A different mint is refused on every instruction that takes one | `wrong_mint_on_claim_and_revoke`, `wrong_mint_source_token_account_of_another_mint`, `wrong_mint_destination_token_account_of_another_mint` |
| Accounts owned by the wrong program (schedule, vault, mint) are refused | `wrong_owner_schedule_account_owned_by_another_program`, `wrong_owner_vault_owned_by_the_system_program`, `wrong_owner_mint_owned_by_the_system_program` |
| The token program and the system program must be the real ones (any other id is refused before a CPI is built) | `fake_token_program_on_every_instruction`, `fake_system_program_on_initialize` |
| The same account cannot play two roles that must differ | `duplicate_accounts_on_initialize`, `duplicate_accounts_on_claim_revoke_and_close` |
| Extra accounts, missing accounts and read-only accounts that must be written are refused | `malformed_instruction_data_and_account_lists`, `read_only_accounts_that_must_be_written` |
| A vault with a delegate or a close authority, or in a frozen or uninitialised state, is refused | `a_vault_with_a_delegate_or_a_close_authority_is_refused`, `frozen_accounts`, `an_uninitialised_vault_is_refused` |
| Token-2022 programs and mints are refused (extensions would change what "the vault holds `total`" means) | `token_2022_is_refused_as_program_and_as_mint` |

### Addresses and initialisation

| Claim | Test |
| --- | --- |
| The schedule address is the canonical program derived address of (creator, mint, id); any other address, including a valid address with a non-canonical bump, is refused | `wrong_pda_a_non_canonical_bump_address_at_initialize`, `wrong_pda_an_address_of_another_creator_or_id` |
| A forged program-owned copy at a non-canonical address is refused on use; so is a stored bump that is not the canonical one | `wrong_pda_forged_schedule_at_a_non_canonical_address`, `wrong_bump_stored_in_the_schedule_does_not_match_the_canonical_bump` |
| An existing schedule is never overwritten, and a program-owned zeroed account is neither initialised over nor usable | `reinitialisation_of_a_live_schedule`, `reinitialisation_of_a_program_owned_account_with_zeroed_data` |
| Wrong-length data is refused | `truncated_and_padded_schedule_data_is_refused`, `programs/vesting_escrow/tests/layout.rs` |
| A closed schedule cannot be used, and sending lamports to its address does not revive it | `a_closed_schedule_cannot_be_used_or_revived` |
| Lamports sent to the schedule address before creation do not block creation (the account is topped up, allocated and assigned) | `lamports_sent_to_the_schedule_address_first_do_not_block_creation`, `an_oversized_donation_to_the_schedule_address_is_kept_not_refunded` (happy_paths.rs) |
| The schedule account is rent exempt when created | `initialize_funds_the_vault_and_records_the_schedule` (happy_paths.rs) |
| A donation of tokens to the vault does not block closing; the surplus goes to the authority | `a_donation_to_the_vault_is_swept_to_the_authority_on_close` (happy_paths.rs) |

### Time and arithmetic

| Claim | Test |
| --- | --- |
| Time comes from the clock sysvar only: data carrying a time, a forged clock account, or the real clock passed as an account change nothing | `clock_manipulation_through_instruction_data`, `clock_manipulation_through_a_forged_clock_account`, and `time_comes_from_the_clock_sysvar_only` in architecture.rs |
| Nothing is claimable before the cliff or before the start | `claim_before_the_cliff`, `claim_before_the_start` |
| The same claim twice, in one transaction or in two, pays once | `double_claim_in_one_transaction` |
| After a revocation nothing more is owed, and a second revocation is refused | `claim_after_a_revocation`, `revoke_twice_and_revoke_after_everything_vested` |
| Revoking a fully vested schedule is refused (nothing to take back) | `revoke_twice_and_revoke_after_everything_vested` |
| Overflowing amounts and timestamps are refused with their own errors | `overflow_amounts_on_initialize`, `invalid_schedule_parameters_are_refused_with_their_own_error` (edge_cases.rs) |
| Stored accounting that is inconsistent with the clock (claimed above total, or above vested) is refused instead of wrapping | `corrupt_accounting_in_the_stored_schedule_is_refused`, `a_clock_that_moves_backwards_cannot_be_used_to_claim_again` (edge_cases.rs) |
| The vested amount is monotonic, never above the total, and rounds down (never overpays); claims sum to the total | `crates/vesting_core/tests/properties.rs` |
| `u64::MAX` amounts and tiny amounts work | `an_amount_of_u64_max_is_released_without_overflow`, `a_one_token_schedule_pays_only_at_the_end`, `rounding_favours_the_vault_on_chain` (edge_cases.rs) |

### Structure

| Claim | Test |
| --- | --- |
| The pure crate has no dependencies, names no Solana type and forbids unsafe code | `architecture.rs`: `the_pure_crate_has_no_dependencies_at_all`, `the_pure_crate_names_no_solana_type_and_forbids_unsafe_code` |
| The program does no schedule arithmetic of its own (clippy `arithmetic_side_effects` is denied, and the wide integer operations are banned outside the pure crate) | `the_program_uses_the_pure_crate_and_does_no_money_arithmetic_of_its_own` |
| `unsafe` appears only in the generated entrypoint | `unsafe_code_exists_only_in_the_entrypoint` |

## Out of scope or accepted (and why)

* **Re-entrancy.** The program makes CPIs only to the SPL Token program and the system program, both fixed ids checked
  before the CPI is built. Neither can call back into this program. State is nevertheless written before each token CPI.
  There is nothing to test; the claim is the id check (`fake_token_program_on_every_instruction`).
* **Freeze authority of the mint.** A mint with a freeze authority can freeze the vault or a payee account. Pay-outs then
  fail with `TokenAccountFrozen`; nothing is lost, it is stuck until the freeze authority thaws it. A revocation pays both
  sides in one instruction, so a frozen beneficiary account blocks revocation until it is thawed (ADR 0004).
* **Program upgrade authority.** Whoever holds the upgrade authority of the deployed program can replace it. The program
  has no say in that; see `docs/audit-checklist.md`, "Upgrade authority". Deploy with the authority revoked or held by a
  multisig, and say so to the users.
* **Compute units, stack and heap.** The tests run the program natively, where these limits do not apply. A run on a
  validator or an SBF test harness is needed before any deployment (README, "What the tests do not prove").
* **Token-2022.** Refused entirely. Supporting it means deciding, per extension, what "vested" means (transfer fees,
  interest-bearing and scaled mints, permanent delegate, transfer hooks, confidential transfers, non-transferable).
* **Wrapped SOL and other native mints.** Not special-cased; not tested. Treat as unsupported.
* **Clock accuracy.** Unix time comes from the cluster's stake-weighted clock estimate; the program assumes it does not
  move backwards and refuses (rather than underflows) if it does.
* **Denial of service by the beneficiary.** The beneficiary can refuse to claim; the authority can then not close. That
  is by design: the vested tokens are the beneficiary's.

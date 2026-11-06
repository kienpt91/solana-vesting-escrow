# Accounts, layouts and addresses

Everything here is derived from `programs/vesting_escrow/src/{state,instruction,pda,processor}.rs`. The test
`schedule_account_layout_matches_the_documented_offsets` (in `programs/vesting_escrow/tests/layout.rs`) pins the offsets below.

## Addresses

| Account | Address | Owner (program) |
| --- | --- | --- |
| Schedule | program derived address, seeds `"vesting" \|\| creator \|\| mint \|\| schedule_id (u64 LE)`, under the vesting program; the canonical bump is found by the program and stored | vesting program |
| Vault | the associated token account of the schedule address for the mint: address derived from `[schedule, token_program, mint]` under the associated token account program | SPL Token program |
| Mint | any initialized classic SPL Token mint | SPL Token program |

The creator is part of the seeds on purpose: nobody else can derive (and so occupy) an address the creator is about to
use. The beneficiary is not part of the seeds, because the beneficiary can change.

The program never takes a bump from instruction data. At creation it calls `find_program_address`, compares the result
with the account that was passed and stores the bump. On every later use it derives the address again from the stored
creator, mint and id, and requires both the address and the stored bump to match (`WrongScheduleAddress` otherwise).

## Schedule account (272 bytes, little endian, no padding between fields)

| Offset | Size | Field | Notes |
| ---: | ---: | --- | --- |
| 0 | 8 | discriminator | ASCII `vestsch1`; the last byte is the layout version. A zeroed account never matches |
| 8 | 1 | bump | canonical bump of the schedule address |
| 9 | 1 | revocable | 0 or 1 |
| 10 | 1 | revoked | 0 or 1 |
| 11 | 1 | reserved | must be 0 |
| 12 | 4 | reserved | must be 0 |
| 16 | 8 | schedule_id | u64 |
| 24 | 32 | creator | signed the creation, funded the vault |
| 56 | 32 | authority | may revoke and close; receives the unvested remainder, any surplus and the rent |
| 88 | 32 | beneficiary | may claim and may propose a successor |
| 120 | 32 | pending_beneficiary | the proposed successor, all zero if none |
| 152 | 32 | mint | |
| 184 | 32 | vault | token account holding the tokens |
| 216 | 8 | total_amount | u64, never zero |
| 224 | 8 | claimed_amount | paid to the beneficiary so far |
| 232 | 8 | returned_amount | returned to the authority by a revocation |
| 240 | 8 | start_ts | i64 Unix seconds |
| 248 | 8 | cliff_ts | i64, `start <= cliff <= end` |
| 256 | 8 | end_ts | i64, `start < end`, `end - start <= 315_360_000` (ten years) |
| 264 | 8 | revoked_at | i64, 0 until a revocation |

Checked on every load (`load_schedule`, used by every instruction but `initialize_schedule`): owner = the vesting program, exact
length 272, discriminator, flags 0 or 1, reserved bytes zero, and the canonical address and bump for the stored creator, mint and id.
Checked by each handler that uses the numbers: that the stored parameters are accepted by `vesting_core::Schedule::new`
(`state.schedule()`) and that `claimed_amount + returned_amount <= total_amount` (inside `claimable`, `after_claim` and
`is_settled`). That is `claim`, `revoke` and `close`; `propose_beneficiary` and `accept_beneficiary` touch only the two keys and
do not re-validate the amounts.
The account holds exactly `Rent::minimum_balance(272)` lamports at creation (more if somebody donated first) and is never
shrunk below rent exemption; only `close` removes lamports.

## Instructions

Data is one tag byte followed by fixed-size fields. The length is checked exactly (trailing bytes are rejected), and no
instruction carries a time: "now" is the clock sysvar, read through the `Clock::get` syscall.

| Tag | Instruction | Data after the tag |
| ---: | --- | --- |
| 0 | `initialize_schedule` | `schedule_id u64`, `beneficiary [32]`, `authority [32]`, `total_amount u64`, `start_ts i64`, `cliff_ts i64`, `end_ts i64`, `revocable u8` (0 or 1) |
| 1 | `claim` | none |
| 2 | `revoke` | none |
| 3 | `close` | none |
| 4 | `propose_beneficiary` | `new_beneficiary [32]` |
| 5 | `accept_beneficiary` | none |

Account lists are exact: fewer accounts is `NotEnoughAccountKeys`, more is `UnexpectedAccounts`.
`S` = signer, `W` = writable.

### `initialize_schedule`

| # | Account | Flags | Checks |
| ---: | --- | --- | --- |
| 0 | creator | S W | signer; pays the rent; owner of the funding account |
| 1 | schedule | W | canonical address for (creator, mint, id); system-owned and empty |
| 2 | mint | | owned by the SPL Token program (Token-2022: `UnsupportedTokenProgram`); initialized |
| 3 | source token account | W | creator's account, right mint, not frozen, balance >= total |
| 4 | vault | W | the associated token account of (schedule, mint); owner = schedule; no delegate, no close authority; not frozen |
| 5 | system program | | the system program |
| 6 | token program | | the SPL Token program |

None of the accounts 0 to 4 may be the same account as another. Effects: creates the schedule account (with `create_account`,
or `transfer` + `allocate` + `assign` if lamports were sent to the address first), writes the state, moves `total_amount`
from the source to the vault with `TransferChecked`, and requires the vault balance to grow by exactly `total_amount`.
The vault itself is created beforehand by the client (`vesting_client::create_vault`, the idempotent associated token
account instruction), in the same transaction.

### `claim`

| # | Account | Flags | Checks |
| ---: | --- | --- | --- |
| 0 | beneficiary | S | equals the stored beneficiary |
| 1 | schedule | W | owned by the program, layout, canonical address |
| 2 | vault | W | equals the stored vault; owner = schedule; no delegate or close authority; not frozen |
| 3 | beneficiary token account | W | stored mint, owner = stored beneficiary, not frozen |
| 4 | mint | | equals the stored mint |
| 5 | token program | | the SPL Token program |

Pays `vested(now) - claimed` (zero before the cliff: `CliffNotReached`; nothing new: `NothingToClaim`; after a revocation:
`ScheduleRevoked`). The schedule address signs the token transfer with its seeds.

### `revoke`

| # | Account | Flags | Checks |
| ---: | --- | --- | --- |
| 0 | authority | S | equals the stored authority |
| 1 | schedule | W | as above |
| 2 | vault | W | as above |
| 3 | beneficiary token account | W | only if the beneficiary is paid (vested and unclaimed > 0): stored mint, owner = stored beneficiary, not frozen. When nothing is owed to the beneficiary it is not read |
| 4 | authority token account | W | stored mint, owner = stored authority, not frozen |
| 5 | mint | | equals the stored mint |
| 6 | token program | | the SPL Token program |

Only if `revocable`, not yet revoked and not fully vested. Pays the beneficiary the vested part that is unclaimed and returns
the unvested remainder to the authority, in one instruction; clears an open beneficiary proposal. Accounts 3 and 4 may be the
same account (one wallet can be both roles); neither may be the vault, the schedule, the mint or the signer, and account 3 is
always a writable account that differs from them even when it is not read.

### `close`

| # | Account | Flags | Checks |
| ---: | --- | --- | --- |
| 0 | authority | S W | equals the stored authority; receives all lamports of the closed accounts |
| 1 | schedule | W | as above |
| 2 | vault | W | as above |
| 3 | authority token account | W | stored mint, owner = stored authority, not frozen |
| 4 | mint | | equals the stored mint |
| 5 | token program | | the SPL Token program |

Only when `claimed_amount + returned_amount == total_amount`. Tokens still in the vault (donations) go to account 3, the
vault is closed with `CloseAccount` signed by the schedule address, then the schedule account is zeroed, its lamports move
to the authority, its owner becomes the system program and its data length becomes 0.

### `propose_beneficiary` and `accept_beneficiary`

| Instruction | # | Account | Flags | Checks |
| --- | ---: | --- | --- | --- |
| propose | 0 | current beneficiary | S | equals the stored beneficiary |
| propose | 1 | schedule | W | as above |
| accept | 0 | proposed beneficiary | S | equals the stored pending beneficiary |
| accept | 1 | schedule | W | as above |

A proposal replaces an earlier one. The default key and the current beneficiary are refused as the new beneficiary.

## Errors

Custom errors have the codes listed in `programs/vesting_escrow/src/error.rs` (0 to 36), one per rejected condition.
Conditions the runtime names itself use the built-in errors: `MissingRequiredSignature`, `InvalidInstructionData`,
`AccountAlreadyInitialized`, `NotEnoughAccountKeys`, `InvalidAccountData`, `AccountNotRentExempt`.

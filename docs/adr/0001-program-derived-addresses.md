# ADR 0001: the schedule and the vault live at program derived addresses

Status: accepted.

## Context

Tokens must be locked in a place that only the program's rules can open. The schedule record needs an address that
nobody can forge, that clients can compute without asking anyone, and that cannot be squatted.

## Decision

* The schedule account is a program derived address with seeds `"vesting" || creator || mint || schedule_id (u64 LE)`.
  Only the program can sign for it, so only the program's rules can move the tokens of its vault.
* The vault is the associated token account of the schedule address. Its address follows from the schedule address and the
  mint, clients can create it with a standard instruction, and the program only has to compare an address.
* The creator is in the seeds so that nobody else can derive, and so occupy, the address of a schedule the creator is about
  to create. The beneficiary is not in the seeds because it can change.
* The bump is never taken from the client: the program finds it, stores it, and on every use derives the address again and
  requires the stored bump to be the canonical one.

## Consequences

* A hostile client cannot pass an address of its choosing for the schedule (`wrong_pda_*`) or the vault (`substituted_vault_*`).
* The derivation costs compute units on every instruction (`find_program_address`). On an SBF target this is a measurable cost
  (about 1,500 units per iteration of the bump search, by the Solana documentation); it has not been measured here because the
  tests do not run SBF bytecode.
* Schedules of one creator and mint are distinguished by a caller-chosen id; reusing an id after `close` is allowed.

## Alternatives considered

* A keypair-owned schedule account: simpler, but then the program cannot sign for the vault and the creator holds a key that
  can be lost or leaked. Rejected.
* A single global vault with per-schedule ledgers: fewer accounts, but one bug exposes every schedule and token accounts of
  different mints cannot be pooled. Rejected.
* Letting the client pass the bump: saves compute, adds a non-canonical-bump attack surface. Rejected.

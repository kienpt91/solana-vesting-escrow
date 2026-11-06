# Adapting this program

This is a reference implementation, not a product. This page lists the usual changes and where they go.

## Change the schedule shape

Everything about the shape lives in `crates/vesting_core/src/schedule.rs`: `Schedule::vested_at`.

* *Stepped release* (monthly tranches): replace the linear branch with `floor(elapsed / step) * step` before the multiplication
  and keep `elapsed < duration`. Keep the three rules: zero before the cliff, `total` from the end, floor rounding. The
  property tests in `crates/vesting_core/tests/properties.rs` apply unchanged (monotonic, never above total, sum of claims).
* *Several cliffs or a back-loaded curve*: the same function with a piecewise definition; add the new shape to the
  count-based oracle test.
* The account layout does not change as long as the shape is a function of (total, start, cliff, end). A shape that needs more
  parameters needs more fields: add them after `revoked_at`, bump the version byte of the discriminator, and extend
  `docs/accounts.md` and `programs/vesting_escrow/tests/layout.rs`.

## Change who may do what

* *Several beneficiaries or a split*: create one schedule per beneficiary. Sharing one vault between schedules would need a
  per-schedule ledger and breaks the rule that the vault holds exactly the unpaid total of one schedule.
* *No revocation at all*: create schedules with `revocable = false`. To remove the instruction, delete `revoke` from
  `processor.rs` and its tag from `instruction.rs`; the attack tests for it become unreachable and should go too.
* *Authority change*: not implemented. If added, make it two-step as well (ADR 0002).

## Use a different token program

Token-2022 is refused (ADR 0005). To support a defined subset: in `token.rs` accept the second program id, parse the mint
extension list, and allow an explicit set of extensions only. Every extension on the allow list needs its own test in
`attacks.rs`, and the vault-growth check after funding must stay.

## Port to Anchor

The program is written so that the account logic ports without redesign (the build tooling does not: see below):

* `ScheduleAccount` becomes an `#[account]` struct (`InitSpace`), the discriminator is Anchor's eight-byte hash, the version byte
  is dropped. Keep the checks that Anchor cannot express: the canonical bump comparison (`seeds`, `bump = schedule.bump`), the
  vault as `associated_token::{mint, authority}`, `token_program: Program<'info, Token>` (this refuses Token-2022), and the
  vault delegate and close-authority checks as `constraint =` lines.
* `validation.rs` disappears into `#[derive(Accounts)]` constraints; `ensure distinct` accounts need explicit
  `constraint = a.key() != b.key()` lines. Anchor does not check that for you.
* `vesting_core` is reused unchanged, which is the point of keeping it pure.
* Keep `clock = Clock::get()?` as the only time source.

## Move to a newer Solana crate family or to an SBF test harness

* Dependencies: replace `solana-program`, `spl-token` and `solana-program-test` pins in the three manifests, regenerate the
  lock file, run `scripts/check-crates-age.mjs`. The API differences are in imports (split `solana-*` crates), in
  `AccountInfo` handling, and in the program-test API.
* SBF: build the program with the Solana toolchain (`cargo build-sbf`) and load the `.so` with a harness that runs bytecode
  (`solana-program-test` with `SBF_OUT_DIR`, LiteSVM or Mollusk are the usual choices). `Env` in
  `crates/vesting_tests/src/lib.rs` is the only place that registers the program. Then add compute-unit assertions per
  instruction, which this repository cannot do. **Do not expect this to be mechanical**: the lock file and `rust-version` of this
  repository are newer than what a Solana 1.18-era toolchain's cargo accepts, so an SBF build needs its own lock file and a
  lowered `rust-version` for the program tree. `docs/sbf-checklist.md` lists what was prepared (the `cdylib` crate type, static
  checks for heap use and large arrays), the expected blockers and the order in which to verify the build.

## Deploy

Choose a real program id (replace `ID` in `lib.rs`, or better, generate a keypair for it and keep it out of the repository),
decide who holds the upgrade authority (revoke it, or use a multisig, and tell your users), run the checklist in
`docs/audit-checklist.md`, and have the program audited. This repository has not been deployed anywhere.

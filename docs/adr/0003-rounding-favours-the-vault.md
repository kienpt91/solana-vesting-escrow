# ADR 0003: rounding always favours the vault

Status: accepted.

## Context

Linear vesting divides `total * elapsed` by `duration`. Integer division has to round somewhere, and every rounding error is
a token that either stays in the vault or leaves it.

## Decision

* `vested(now) = floor(total * (now - start) / (end - start))` between the cliff and the end, computed in `u128` with
  checked operations; zero before the cliff; exactly `total` from the end on.
* The beneficiary can never receive more than the exact linear amount; the difference stays in the vault until the end
  time, at which the full total is released, so no dust is left behind and the claims sum to exactly `total`.
* A revocation uses the same function: `to_beneficiary = vested - claimed`, `to_authority = total - vested`, so
  `claimed + to_beneficiary + to_authority == total` with no remainder created or lost.

## Consequences

* Tiny amounts vest late: one token over 1,000 seconds pays out at the end only (`a_one_token_schedule_pays_only_at_the_end`).
* The properties are tested without an oracle that shares the implementation: monotonic, never above total, floor exactness,
  claims sum to total, conservation on revocation, a count-based oracle on every small schedule
  (`crates/vesting_core/tests/properties.rs`).
* Rounding up would let a sequence of tiny claims drain more than the exact schedule in total on some inputs; rounding to
  nearest has the same flaw. Floor is the only rule for which "never overpay" holds at every instant.

## Alternatives considered

* Round to nearest, or up: rejected as above.
* Per-second accumulation with a remainder carried in state: exact, but needs more state and more code to get wrong.

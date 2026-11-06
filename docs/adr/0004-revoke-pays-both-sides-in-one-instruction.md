# ADR 0004: a revocation pays both sides in one instruction

Status: accepted.

## Context

When the authority revokes, two things must happen: the vested part that was not yet claimed belongs to the beneficiary,
the unvested rest goes back to the authority. Either the revocation could only record a "frozen" vested amount and let the
beneficiary claim later, or it could pay both sides at once.

## Decision

`revoke` pays both: the vested, unclaimed part to a token account of the beneficiary and the unvested remainder to a token
account of the authority, in one instruction. Afterwards the schedule is settled (`claimed + returned == total`) and
`close` is possible.

## Consequences

* No state is left in which tokens wait for a beneficiary action after a revocation; `claim` after a revocation is refused.
* The revoking transaction needs a token account of the beneficiary. If that account is frozen (the mint's freeze authority
  froze it) the revocation fails until it is thawed. The alternative (pull payment) avoids that, at the price of an extra
  state and a longer-lived vault. Documented as an accepted trade-off in the threat model.
* Destination owners are checked against the stored beneficiary and authority, so the payout cannot be redirected
  (`revoke_payout_cannot_be_redirected`).
* The same wallet may be both roles; both payouts then go to one account (`the_authority_and_the_beneficiary_may_be_one_wallet`).

## Alternatives considered

* Record the vested amount and let the beneficiary pull it: rejected for the extra lifetime and state.

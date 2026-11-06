# ADR 0002: the beneficiary changes in two steps

Status: accepted.

## Context

A beneficiary may lose a key, or move to a new wallet. A one-step "set beneficiary" instruction would let a typo, or a
compromised signer, hand the schedule to an address that can never claim (or to an attacker) without any confirmation from
the receiving side.

## Decision

* `propose_beneficiary` is signed by the current beneficiary and records a pending key. Nothing else changes.
* `accept_beneficiary` is signed by the pending key and makes it the beneficiary. A new proposal replaces a pending one.
* The authority and the creator have no say. Only the beneficiary may redirect its own entitlement; the authority's powers are
  revoke and close.

## Consequences

* A proposal to a wrong address is harmless: it can never be accepted, and the real beneficiary can overwrite it.
* A stranger can neither propose nor accept in someone else's place (`beneficiary_swap_*`).
* Claims always pay a token account owned by the current beneficiary, so a successor is paid into its own account.
* There is no instruction to cancel a proposal; proposing again replaces it. Proposing the default key is refused, so a
  proposal cannot be "cleared" to zero by mistake either.

## Alternatives considered

* One step: rejected for the reasons above.
* Timelocked change: more state and more states to test, little gain for a schedule that pays out over weeks. Not needed here.

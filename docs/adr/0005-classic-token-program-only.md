# ADR 0005: only the classic SPL Token program

Status: accepted.

## Context

Token-2022 mints can carry extensions that change what a transfer does: transfer fees (the vault receives less than was
sent), transfer hooks (arbitrary code during a transfer), a permanent delegate (someone can take the vault's tokens),
interest-bearing and scaled amounts (what "total" means changes with time), default account state, pausing, confidential
transfers. Each needs a decision about what "vested" means, and a test.

## Decision

Token-2022 is refused: as the token program (`UnsupportedTokenProgram`), as the owner of the mint, as the owner of a token
account. The vault balance must grow by exactly the funded total, which is a second line of defence against fee-like
behaviour.

## Consequences

* A smaller surface and no extension parsing. The cost is that Token-2022 mints cannot be vested with this program.
* Supporting Token-2022 later is a deliberate change: parse the mint's extension list, allow only an explicit set of
  extensions, and extend the attack suite per extension. `docs/adapting.md` outlines it.

## Alternatives considered

* Accept Token-2022 mints that carry no extension: possible, but the program would then have two token programs to check on every
  instruction and would have to prove "no extension" from the mint's account data. Deferred, not rejected on principle: the
  cost is code and tests, and `docs/adapting.md` outlines it.

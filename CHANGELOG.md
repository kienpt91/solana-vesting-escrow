# Changelog

All notable changes. Versions follow `[workspace.package] version` in `Cargo.toml`; a release is a tag `vMAJOR.MINOR.PATCH`
(see `RELEASING.md`).

## Unreleased

### Added

- `vesting_core`: pure schedule arithmetic with checked `u64`/`u128` operations: validation (cliff, end, ten-year bound),
  `vested_at` (floor rounding), `claimable`, `after_claim`, `is_settled` and the revocation split.
- `vesting_escrow`: the program. Instructions `initialize_schedule`, `claim`, `revoke`, `close`, `propose_beneficiary`,
  `accept_beneficiary`; the schedule account at a program derived address with a stored canonical bump; the vault as the
  associated token account of the schedule address; a hand-written account-validation layer; the classic SPL Token program only.
- `vesting_client`: address derivation, instruction builders and account decoding.
- `vesting_tests`: the in-process harness and the suites: happy paths, edge cases, attacks (one precise error each),
  architecture rules. Seeded property tests of the schedule math live with `vesting_core`.
- Documentation: threat model, audit checklist, account layouts, decision records, adaptation notes.
- Tooling: container wrapper `scripts/rs.sh` (pinned image digest, network none after the fetch), secret scanner, commit hygiene
  guards, demo and coverage scripts, CI and a tag-triggered release workflow (source tarball, SHA256SUMS, provenance
  attestation).

### Changed after an independent review

- The demo fails when cargo fails or when an attack row is missing (`scripts/attack-table.sh`, tested by `scripts/test-demo.sh`);
  it used to drop the first row of every attack test. The recorded run counts the real number of hostile transactions and
  README, SECURITY and the architecture tests agree with it.
- Errors in the attack evidence name the program that returned them (a token program error was printed with a vesting error name).
- `revoke` reads the beneficiary's token account only when the beneficiary is paid, and clears an open beneficiary proposal.
- The program crate has the `cdylib` crate type; `docs/sbf-checklist.md` lists what is prepared for an SBF build, what is checked
  statically (no allocation in handlers, no large stack array, value sizes) and what remains unverified.
- `scripts/rs.sh --net` accepts exact commands only; the secret scanner knows Solana keypair arrays, base58 key-length values and
  keypair file names; `.gitignore` ignores keypair files in every directory.
- `scripts/check-lock.sh`, `scripts/test-lock.sh` and `scripts/verify-lock.sh` check `Cargo.lock`; `docs/lockfile.md` records how it
  was made, including the hand-edited `wasm-bindgen` entries, and the CI `lockfile` job runs the verification.
- Documentation brought in line with the code: what is checked on every load, the stack-frame sentence, the same-transaction
  revival test, the findings table of the audit checklist.

### Known limitations

- Not audited, not deployed. The vesting program is tested as a native build; no SBF build exists in this repository's
  tooling, so compute units, stack and heap are unmeasured.
- Token-2022 and wrapped SOL are unsupported.

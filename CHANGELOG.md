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

### Known limitations

- Not audited, not deployed. The vesting program is tested as a native build; no SBF build exists in this repository's
  tooling, so compute units, stack and heap are unmeasured.
- Token-2022 and wrapped SOL are unsupported.

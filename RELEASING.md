# Releasing

A release is a git tag `vMAJOR.MINOR.PATCH` pushed to GitHub. The `release` workflow (`.github/workflows/release.yml`) does the
rest. Nothing is published to a package registry (every crate has `publish = false`).

## Cut a release

1. On `main` with a green CI, set the new version in the `[workspace.package]` table of `Cargo.toml`, bring `Cargo.lock` to the
   same version for the four workspace crates (edit the four `version = "..."` lines of the `vesting_*` packages in `Cargo.lock`
   by hand; `cargo update` is deliberately not used in this repository, it would move the pinned dependencies), update
   `CHANGELOG.md`, and commit those files.
2. Tag that commit and push the tag:

   ```sh
   git tag -a v0.2.0 -m "v0.2.0"
   git push origin main v0.2.0
   ```

The workflow runs only for a pushed tag that starts with `v`. It stops before building anything unless

- the tag is exactly `v` plus the version in the manifest (`scripts/check-release-tag.sh`), and
- the tagged commit is in the history of the default branch.

Then the `verify` job repeats what CI does (secret scan and hygiene on the tree and the history, format check, clippy, the layered
tests, the host build of the program), and the `release` job packages the source of the tagged commit, writes `SHA256SUMS`,
creates a build-provenance attestation for the files and creates the GitHub release with the built-in `GITHUB_TOKEN` (no other
secret is involved). The release notes are the subjects of the commits since the previous tag (`scripts/release-notes.sh`).
A tag with a `-` in it (`v0.2.0-rc.1`) is marked as a pre-release.

To try the packaging without releasing anything, run the `release` workflow by hand (Actions, release, Run workflow). A manual
run keeps the files as a workflow artifact for seven days; it never creates a release or an attestation.

## What a release contains

- `solana-vesting-escrow-VERSION-source.tar.gz`: `git archive` of the tagged commit (same commit, same bytes).
- `SHA256SUMS`: the SHA-256 of every file above.

There is no program binary. The SBF toolchain is not part of this repository's tooling policy, so a deployable `.so` cannot
be built here, and a host build of the program would be misleading as a release artefact. When a release is meant to be
deployed, build the SBF binary yourself from the tagged source with the Solana toolchain and publish its hash next to the tag.

## Verify a download

```sh
sha256sum -c SHA256SUMS            # macOS: shasum -a 256 -c SHA256SUMS
gh attestation verify FILE --repo OWNER/solana-vesting-escrow
```

The checksum only shows that the file is the one listed. `gh attestation verify` checks the signed provenance statement that
GitHub recorded when the workflow built the file: that it was produced by this repository's `release` workflow, from the tagged
commit. Do both. (Attestations are available for public repositories.)

## Before the first tag

The workflows have not been run on GitHub from this checkout (the repository has not been pushed). Run the `release` workflow
manually once and read its log before relying on it.

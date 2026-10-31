# allowit-sdk

## Artifact storage

Keep code, required build/test inputs and concise API/usage documentation in this repository. Plans, design reports, reviews, run evidence, demo recordings and screenshots belong in the main project wiki repository under `instance/artifacts/`; capture their original Markdown sources with the wiki source-ingestion tooling. Never commit those artifacts in this subproject or its feature branches. Link to the canonical wiki artifact instead.

## Before pushing

Read the affected workflows in `.github/workflows/`. Run their affected test and
validation commands locally, including their feature flags and selectors. Include
workflow and test-input changes when selecting checks. Resolve failures before
pushing. Preserve existing reviews and deployment restrictions.

All packages share one Cargo workspace, one `Cargo.lock` and one version
(`[workspace.package]`). Packages in this repository depend on each other by
path and are never pinned. Select them with `-p allowit-sdk`,
`-p allowit-sdk-engine`, `-p allowit-cli`, `-p allowit-policy-template` or a
rail host crate under `rails/` (the `rails` job in `.github/workflows/fast.yml`
lists them). Test `allowit-sdk` in its own cargo invocation: a workspace build
unifies the CLI's serde_json features into core.

Pins point one way. The rail repositories depend on this SDK; this SDK does
not depend on Solana or Tempo. The only cross-repository git dependency is
`allowit-stellar-wallet`, pinned by full `rev` in the root
`[workspace.dependencies]`. `[workspace.metadata.allowit.track]` says which
revision it must be pinned to. Run `scripts/build.sh` before pushing: it fails
on a stale pin and builds the workspace and the Wasm module.
`scripts/build.sh --update` rewrites stale pins and `Cargo.lock`. The root
`[patch]` builds `allowit-stellar-wallet` against `core/`, because it depends
on `allowit-sdk` itself. Remove the patch and the pin when that crate moves into
`rails/` after `stellar-core` merges.

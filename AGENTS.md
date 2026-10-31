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
path. Select them with `-p allowit-sdk`, `-p allowit-sdk-engine`,
`-p allowit-cli` or `-p allowit-policy-template`. Test `allowit-sdk` in its own
cargo invocation: a workspace build unifies the CLI's serde_json features into
core.

Rail crates are git dependencies with a full `rev` in the root
`[workspace.dependencies]`. `[workspace.metadata.allowit.track]` says which
revision each rail repository must be pinned to. Run `scripts/build.sh` before
pushing: it fails on a stale pin and builds the workspace and the Wasm module.
`scripts/build.sh --update` rewrites stale pins and `Cargo.lock`. The root
`[patch]` builds the rail crates against `core/`, because they depend on
`allowit-sdk` themselves.

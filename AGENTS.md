# allowit-sdk

## Artifact storage

Keep code, required build/test inputs and concise API/usage documentation in this repository. Plans, design reports, reviews, run evidence, demo recordings and screenshots belong in the main project wiki repository under `instance/artifacts/`; capture their original Markdown sources with the wiki source-ingestion tooling. Never commit those artifacts in this subproject or its feature branches. Link to the canonical wiki artifact instead.

## Before pushing

Read the affected workflows in `.github/workflows/`. Run their affected test and
validation commands locally, including their feature flags and selectors. Include
workflow and test-input changes when selecting checks. Resolve failures before
pushing. Preserve existing reviews and deployment restrictions.

All crates share one Cargo workspace and one `Cargo.lock`. Select packages with
`-p allowit-sdk`, `-p allowit-sdk-engine`, `-p allowit-cli` or
`-p allowit-policy-template`. Test `allowit-sdk` in its own cargo invocation: a
workspace build unifies the CLI's serde_json features into core. Rail crates
are git dependencies pinned by exact commit in the root `[workspace.dependencies]`;
the root `[patch]` maps their `allowit-sdk` dependency to `core/`.

For changes under `verification/`, including documentation, run:

```sh
python3 -m unittest discover -s verification/tests
```

Before changing verification documents, check whether observations, locks or
receipts bind their hashes, including references from other verification folders.
Preserve hash-bound historical inputs. Do not change recorded hashes to make a
document edit pass.

The Solana native check (`check_native.py`, `native-correspondence.json`,
`native-source-lock.json` and the Lean inputs it binds) also lives in
allowit-contracts-solana. The copies here stay because the other verification
folders bind them by hash.

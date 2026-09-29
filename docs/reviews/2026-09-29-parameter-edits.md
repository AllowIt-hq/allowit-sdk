# Policy parameter edits

Independent Claude Code review completed with actual model `claude-opus-5-5`, session `536ade94-31a9-4a33-be2f-e1d1282e7356`. The initial review covered `b743ccd` through the implementation in `7847823`; the closure covered the changes now in `6268d7e`.

The material finding was partial threshold metadata: a supported literal comparison could be edited while another use of the same semantic result was omitted. The editor now requires every variable read to be a supported direct literal comparison; mixed reversed, computed or aliased uses expose no controls and reject the edit. The independent reviewer replayed the original counterexample and confirmed closure, with no remaining material findings.

Four-argument `None` stores no numeric boundary. Changed literals use canonical decimals; unchanged values preserve their original bytes. Both limitations are explicit in the API documentation. Threshold metadata describes comparisons, not branch outcomes or statistical confidence.

Verification: 75 Rust tests including 20 parameter-edit regressions; clippy; no-default-feature/profile checks; release WASM and the WASM ABI, semantic-evidence, parser/resource and memory checks passed. Tests and review are evidence of implementation behavior, not a proof of natural-language coverage or classification accuracy. Go/TypeScript consumers have a separate app review.

Review receipts: `/tmp/allowit-sdk-ast-review.json` and `/tmp/allowit-sdk-ast-closure.json`; both completed with verified model usage.

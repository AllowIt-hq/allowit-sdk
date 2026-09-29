# Halt source spans

Independent Claude Code review: Claude Opus 5.5 (`claude-opus-5-5`, verified in result modelUsage), session `3df7fdfc-38ac-4828-afe1-3029b9a53878`.

Reviewed SDK implementation commit `32bde55e4307459e900fe9179e5d94c297338c1c`: evaluator/types diff, trace tests and supporting compiler/validation/spending source. No blockers. Evaluation outcomes, step limits, authority gates, input keys and IR hashes remain unchanged; new optional spans identify the innermost halted call or its containing statement.

Validation separately ran 46 native tests, clippy with warnings denied, the no-default/feature checks and WASM compilation. The app integration also tested repeated calls and a Unicode prefix before the blocked step. Review was read-only and did not independently rerun tests.

Reader compatibility and statement-wide fallback spans are documented in the README. Bounds-check diagnostic spans from externally supplied IR. This review covers the SDK only; it does not cover app HTTP, action CLI, hosted controller or standalone-engine integration.

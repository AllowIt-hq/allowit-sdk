# Requirements contract and Lean review

Independent review used Claude Code session `765acce9-d31e-4355-b84d-1e34b4d9ba75`. The returned `modelUsage` and `canonicalModel` both identify `claude-opus-5-5`. This is separate from the Go skill author's session.

The first pass reviewed requirements extraction and artifact integrity at `f454e83`, the settled Lean model committed as `93694ec`, and the application architecture/language comparison. It found no material SDK or mathematical defect. Documentation was corrected to distinguish future per-policy obligation checks from today's AST-edit/recompile path and to describe classification output as a point score. The checker now accepts only the two standard logical axioms used by the proofs and rejects `debug.skipKernelTC`.

Local verification completed independently of the review:

- 84 Rust tests, Clippy and supported feature profiles passed.
- Release WASM checks passed, including ABI/no host imports, bounded parsing, decision traces, runtime limits and source bindings. Application artifact SHA-256: `b4616f47edc3d7ee1da7356af3284191cb0c513b938b44c65fc391d219c228eb`, built from `f454e83`.
- `verification/lean/check.py` checked all 13 theorems with pinned Lean 4.11.0 and audited their dependencies. Only `propext` and `Quot.sound` occur; some theorems use neither. No unfinished proof or application-specific axiom is present.

The reviewer read source and did not execute checks. The Lean result applies only to the documented model. Neither review nor tests establish that Rust/Go implements that model, that natural-language requirements were fully captured, that classifications are accurate or calibrated, or that an action settled on a live network.

The review receipt is retained locally at `/tmp/allowit-formal-review.json`. No push or deployment was performed.

# Exact decimal helpers review

Reviewed source commit: `65a9bfe12f5abcbc7786a37b15f7f7a34f708dbd`.
Reviewer: Claude Code, explicitly selected and returned `claude-opus-5-5`.
Scope: shared Rust compiler, decimal parsing, prelude, registry, type validation, evaluator and targeted tests. The request supplied source contents with per-file SHA-256; tools were disabled. App source and agent-skill content were excluded. This is static review, separate from executed tests.

Result: no material correctness or security findings. The reviewer checked exact conversion/overflow, inclusive comparisons, short-circuit subtraction, bounded lowering, source spans, facade agreement and absence of new contract opcodes. Remaining low-severity observations: old compilers reject new helper source despite the unchanged IR registry version; the full Rust facade accepts constructs the restricted compiler rejects; decoded Rust string escapes are accepted; compile/facade invalid-literal error codes differ. Source must be compiled by the SDK revision named above or later. Hosts pin an exact SDK artifact rather than inferring source-feature support from the IR version.

Local validation: all 34 Rust integration tests; clippy with warnings denied; no-default-features library check; import-free WebAssembly and its ABI, adversarial parsing, memory, semantics and decimal-helper suite. Contract profile tests evaluate lowered IR using the same interpreter; public-chain deployment is separate.

# Review: decimal helpers (`usdc`, `percent`, `amount_at_most`, `within_percentage_points`)

**Result: I found no material correctness or security findings.** This is a static review. I did not execute anything or run the target tests. `lsp.rs` and `protocol.rs` were not provided, so hover and protocol handling are out of scope.

## What I checked, and why each holds

**Overflow (`readability.rs:4-35`)**
- The 24-byte cap and ASCII-digit checks run before parsing.
- Integers too large for `u64` are rejected by `parse::<u64>`.
- `checked_mul` and `checked_add` guard the scaled value.
- `decimals - value_fraction_len` cannot underflow, because `fraction.len() <= decimals` is checked first.
- Fullwidth digits, sign, exponent, `1.`, `.1` and `1..1` are all rejected.
- `percentage_bps` caps at 10,000 after exact scaling, so `100.001` is already rejected by the length check.

**Return-gap arithmetic (`compiler.rs:~140`)**
- The lowering produces `c >= b || (b - c) <= gap`.
- The runtime short-circuits `||` (`runtime.rs`, `Binary` arm), so subtraction only runs when `b > c`. It cannot underflow at any `u64` value.
- Because of the operand restriction, `candidate` and `benchmark` can only be `Variable` or literal nodes. Duplicating them therefore has no side effects and no expansion.
- Nested helpers such as `percent(..)?` or `context_u64(..)?` as operands are rejected before recursion.

**Parser escape hatches.** Every alternate form I tried fails closed:
- `usdc("1")` (no `?`), `(usdc("1"))?`, `usdc("1").await?` and `usdc("1")??` all fall through to an `Expr::Call "usdc"`, `Expr::Try{Integer}` or `Await` node, which `validate_program` rejects.
- Turbofish and qualified paths are rejected by the preflight (`:`/`<`) and by `simple_path`.
- Suffixed strings and byte or C-string literals are rejected.
- In `amount_at_most`, `ctx` cannot be rebound, because `validation.rs` rejects `name == "ctx"` in every scope. The lowered `ctx.amount_units` therefore always refers to the real context.

**Resource bounds**
- Lowering can grow a helper to about 9 IR nodes and about 3 extra levels of depth.
- `validate_program` re-checks `MAX_NODES` and `MAX_DEPTH` on the IR. The runtime step cap (`2 × MAX_NODES`) also exceeds any validated tree, since there are no loops.
- The 1,024-token cap limits the total number of helpers to roughly 128.
- The worst outcome is a compile-time rejection near the limits, never unbounded work.

**Facade equivalence**
- In `amount_at_most`, the facade's token check matches `validate_context`, which rejects any token other than `USDC` before evaluation in both `evaluate` and `evaluate_ir`.
- Inclusive `<=` semantics match.
- Zero is allowed for comparisons and never reaches `set_cap` or `amount_units`.

**No new opcode**
- Helper names never appear as `Expr::Call` in accepted IR.
- A serialized IR containing `usdc` is rejected by `validate_program`, so contracts see nothing new.
- Cap pre-enforcement in `run()` is unchanged.

**Source mapping**
- Helper `CallSite`s use the function-name path span and are deduplicated by start offset.
- Helpers never appear in IR, so they cannot collide with `walk_block` entries.
- Workflow blocks come from statement spans that are unchanged by lowering. Statements containing helpers map to `custom` blocks, and `function(name).expect(..)` cannot be reached with a helper name.

## Low-severity notes (not material)

1. **Cross-version compatibility (`types.rs` `LANGUAGE` / `REGISTRY_VERSION`).**
   - The set of accepted source grew, but both version strings are unchanged.
   - `runtime::evaluate` recompiles `policy.source`. An older SDK will therefore return `INVALID_ARTIFACT` for a helper-using policy that has the same `language` and `registry_version`. `evaluate_ir` still works.
   - This fails closed, but clients cannot detect it by version. Bumping `REGISTRY_VERSION` would change every IR hash, so consider a separate source-language minor version or documenting a minimum SDK version.
2. **Facade divergence.** These cases fail closed or are only confusing; neither is a vulnerability.
   - `amount_at_most(ctx, "10")?;` as a statement compiles in rustc (the bool is silently discarded). The AllowIt compiler rejects it, but with the message "Use ? to check every predefined function result", which is misleading. An authoring hint such as "wrap in `if !…? { return fail(..) }`" would help.
   - `let usdc = 5; … usdc("1")?` is accepted by the AllowIt compiler, but rustc rejects it because the local shadows the function.
3. **Error code (`compiler.rs`, `.map_err(|e| error(literal.span(), e.message))`).** The `INVALID_LITERAL` code is replaced with `INVALID_POLICY`, whereas the facade reports `INVALID_LITERAL`. This is cosmetic.
4. **Literal escapes (hardening).**
   - `usdc("2\x35")` and raw strings are accepted, and their decoded value is used.
   - Workflow `source` shows the escaped text, so this is not hidden. Still, rejecting literals whose token text differs from the decoded value (escapes and raw strings) would keep the displayed amount identical to the enforced amount.

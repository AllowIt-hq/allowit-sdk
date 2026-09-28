# Stellar transport source reviews — 2026-09-28

Reviewer: Claude Code with explicitly selected and completed `claude-opus-5-5`, verified from each response's `modelUsage`. Session: `68031d6f-8b3c-4799-9a0d-740947310371`. The user explicitly approved sharing these private contract/evaluator sources for this review.

These reviews cover the bounded binary policy transport and the Stellar integer JSON input parser. The exact source/IR hashes, compiler and owner authorization, evidence binding and shared contract evaluator remain enforced. Actual compiled-artifact execution is a separate gate recorded in the runtime evidence. No review establishes public-chain deployment or wallet acceptance.

## Binary transport

Candidate: `58b0374204ef7d4a70fbda4eca2d015dd121e98e`. Review response SHA-256: `2c795180df88e2c8bd9c4f2bf144d16b5235c5ae98058eaf65a9bb476ca8fdec`.

I found no material source defects in the change from `6e1865c` to `58b0374`. This was reviewed on Claude Opus 5.5 (`claude-opus-5-5`). HEAD is `58b0374204ef7d4a70fbda4eca2d015dd121e98e`, the working tree is clean, and I didn't build or edit anything.

The delta touches 7 files: `binary.rs`, `lib.rs`, the artifact CLI, `tests/binary.rs`, the Stellar `lib.rs` and `test.rs`, and the README. The workflow and the shared SDK evaluator did not change in this range.

## What I checked

- **Memory, stack and resource limits:**
  - `decode` rejects input over 8192 bytes or with the wrong magic before parsing anything (`binary.rs:40`).
  - Node count and depth are checked as the first step of every statement and expression, before any recursion or allocation (`binary.rs:266-273`, `290`, `329`). Recursion therefore stops at depth 8.
  - `list_count` limits each list to the remaining node budget and the remaining bytes, and the lists grow as items are read rather than being pre-sized. It can't underflow, because decoding stops the moment the count passes 256.
  - Every string length has to fit in the remaining input, so total allocation is bounded by about 8 KiB plus the node overhead.
- **Malformed encodings:**
  - Every tag is matched exactly, and booleans and the optional tag accept only 0 or 1.
  - Integers, counts and spans are fixed-width little-endian, strings must be valid UTF-8, and trailing bytes are rejected (`binary.rs:58`).
  - The format has exactly one encoding per artifact, so the same artifact can't be sent as different byte strings.
  - The decoder reads each variant's fields in the same order the encoder writes them.
- **Source and IR binding:**
  - The chosen bytes are hashed and compared with `artifact_hash` before decoding (`lib.rs:290-299`). Owner and compiler authorization still cover those bytes.
  - After decoding, `validate_decoded_artifact` still checks every metadata field and recomputes `canonical_ir_hash`, which runs `validate_program` (`lib.rs:326-343`).
  - The reconstructed JSON must still be at most 8192 bytes (`lib.rs:310-316`), so switching to binary doesn't admit a larger policy.
  - A JSON artifact can't be sent to Stellar, or a binary one to Solana: the magic check and the JSON parse each reject the other format.
- **Consistency with the chain limits:** the decoder assigns depth exactly the way `validate_chain_program` does, for every statement and expression kind, and counts nodes the same way. `encode` runs that same check first, so the encoder and decoder accept the same set of programs.
- **Evaluation:** `prepare_binary_execution` differs from the JSON path only in which decode function it passes to `prepare_execution_with`. The request binding, nonce, budget, canonical-context, evidence, interval and `evaluate_ir(Profile::Contract)` steps are unchanged, and user input still fails.
- **Authorization and effects on Stellar:** only the two function names changed (`stellar/src/lib.rs:208`, `292`). The authorization order, latest-revision check, asset check and transfer-then-record order are the same.
- **Tests:** the new tests cover the following, and I checked that each one tests what it claims:
  - the JSON round trip, byte for byte
  - encode after decode giving the same bytes
  - JSON and binary producing the same decision across amounts and confidence bounds
  - every truncated prefix of an artifact
  - trailing bytes, an oversized length, bad UTF-8 and a wrong magic
  - depth 64, 257 nodes, an unknown tag and an invalid boolean
  - a forged `ir.version` that still has a matching digest
  - all Stellar native and Wasm fixtures now going through the binary path

## Still pending (not source findings)

- The binary Soroban CI hasn't run yet. That run has to show that uploading the optimized Wasm plus activating and executing the maximum 8192-byte, depth-8 semantic policy fits within the default 100M CPU budget.
- The binary path still includes serde_json, for `canonical_ir_hash`, the JSON size check and parsing `runtime_context`. How much the Wasm actually shrinks is for that CI run to show.
- There is still no public-chain deployment or wallet acceptance.

## Integer numeric profile

Candidate: `928508d6df6c8c625ce989dbb895b7b347f244ae`. Review response SHA-256: `b5bc2e89de18d7a2f942399f98db1ec8aaa87118d6189dc26bbd9c91463c2ff1`.

I found no material findings in `58b0374..928508d`. This was reviewed on Claude Opus 5.5 (`claude-opus-5-5`). HEAD is `928508d6df6c8c625ce989dbb895b7b347f244ae` with a clean working tree. The change touches 6 files, and I didn't build or edit anything.

## Numeric canonicalization

I checked this against the pinned serde_json 1.0.150 source (`de.rs:937-957`):

- **Integers in range:** with `arbitrary_precision`, whole numbers that fit in u64 or i64 still come back as ordinary u64 or i64 values. They serialize to the same text as the standard build, and the evaluator's `context_u64` → `as_u64()` reads them the same way. For accepted numbers, both builds mean the same thing.
- **Fractions, exponents and out-of-range numbers:** these are kept as their raw text, so the canonical check (`to_string == input`) passes for text like `2.0` or `2e0`. The new `validate_integer_context` then rejects them because they aren't exact integers (`lib.rs:498-529`). The tests cover `2.0`, `2e0`, u64::MAX + 1, a nested `0.5`, and i64::MIN − 1.
- **`-0`:** this is the only integer text whose meaning could differ. It parses as i64 0 and re-serializes as `"0"`, so the canonical check rejects it. The standard build also rejects it (it becomes `-0.0`). Both builds behave the same.
- **Accepted inputs:** Stellar accepts exactly the integers written in canonical form within the u64 and i64 ranges. Other JSON value types behave as before, apart from the stricter number rule.

## Request hash, evidence and semantics

- `request_hash` still hashes the exact `runtime_context` bytes through Borsh. Evidence intervals are Borsh integers and never go through serde. The binding hasn't changed.
- The integer check runs after the canonical and size checks and before the evidence checks. Every failure fails closed, and none can turn into a pass.
- The artifact spans and `Expr::Integer` values are in-range u64, so the internally tagged IR still deserializes correctly with `arbitrary_precision`. `canonical_ir_hash` uses serialization, which the feature doesn't change. The shared SDK code doesn't use f64 or `Number` anywhere.

## Allocation limits

The new check walks the value with an explicit stack instead of recursion. Depth is capped at 8 and total entries at 128, the same limits the SDK applies later. Each number allocates one string, and the total is bounded by the 1 KiB context.

## Feature unification

- The feature is enabled only by the Stellar crate's dependency on common.
- Solana and `sbf-tests` are separate workspaces with their own lockfiles, and common's default feature set is empty. The Solana graph isn't affected.
- In the Stellar graph there is only one serde_json version (1.0.150). The two host-only dependencies in that graph, `soroban-env-macros` (a proc-macro) and `crate-git-revision` (a build dependency), resolve their features separately and don't pick up the flag. The native Stellar tests and the Wasm build therefore both run with the flag, so they parse numbers the same way.

## Provenance

The README statement is accurate. The new CI step runs the common crate's tests with `--features integer-json`. Its log is named `stellar-codec-tests.log`, which only describes the purpose; it doesn't claim a Stellar or VM run.

**Non-material rail difference:** Stellar rejects context floats that Solana accepts. This is documented and fails closed.

## Still pending

- The Soroban CI hasn't run on this commit. It has to show that the Wasm contains no float opcodes and that upload, activation and execution fit the unrelaxed budgets.
- There is still no public-chain deployment or wallet acceptance.

## Bounded JSON parser

Candidate: `e7ccdc406e107355edafc23035d082bfe3b0056a`. Review response SHA-256: `c08fc6cde87100f734829defe5f9064d67107a86571e0617365c8456051a3ae5`.

I found no material findings in `928508d..e7ccdc4`. This was reviewed on Claude Opus 5.5 (`claude-opus-5-5`). HEAD is `e7ccdc406e107355edafc23035d082bfe3b0056a` with a clean working tree. The change touches four files: `integer_json.rs`, `common/src/lib.rs`, `tests/binary.rs` and the README. I didn't build or edit anything.

## Parser (`contracts/common/src/integer_json.rs`)

- **Structure:**
  - Input over 1024 bytes is rejected up front (`:9`), and any bytes left over after the top-level value are rejected (`:18`).
  - Keys must be quoted strings, and each member or element must be followed by exactly `,` or the closing bracket. So trailing commas, empty members, and unclosed or truncated input all fail.
  - `true`, `false` and `null` must match exactly.
  - Whitespace is never accepted anywhere. That is stricter than JSON, but `serde_json::to_string` never emits whitespace, so the later canonical check would reject it anyway.
- **Stack and allocation:**
  - Depth is checked on entry to every value, including scalars, before any recursion (`:60`), so the parser recurses at most nine levels deep.
  - Object members and array elements are counted together and capped at 128 before each child is parsed (`:51-58`).
  - Everything allocated is bounded by the 1 KiB input.
  - The depth and entry numbering match the SDK's `validate_runtime_context`, so both enforce the same limits.
- **Numbers:**
  - A leading zero followed by more digits is rejected, and so is a `-` with no digit after it.
  - u64 accumulation uses checked arithmetic, and `.`, `e` or `E` after the digits is rejected.
  - i64::MIN is handled explicitly. Every other negative value is at most i64::MAX in magnitude before negation, so it can't overflow.
  - `-0` produces 0, which re-encodes as `"0"`, so the canonical check rejects it.
- **Strings, escapes and UTF-8:**
  - Raw control bytes 0–31 are rejected.
  - The input is already a `&str`, and `"` and `\` are ASCII, so raw runs between them are always whole UTF-8 code points. The final `String::from_utf8` is an extra safety check.
  - Only the eight standard JSON escapes are accepted, and `hex4` requires exactly four hex digits and can't overflow.
  - A high surrogate must be followed by `\u` and a low surrogate. A lone low surrogate fails `char::from_u32`. The combining arithmetic can't underflow.
  - Non-canonical spellings are refused by the canonical check. serde_json writes raw UTF-8, lowercase `\u00xx`, and never escapes `/`. So `\/`, `\u001F` and an escaped emoji all differ from the input and fail.
- **Duplicates:** `Map::insert` returning an existing value is rejected (`:79`). Keys that are empty or over 128 bytes are rejected, the same as the SDK does later.

## Parity with the standard path

- For every valid input, the parser builds the same `Value` that `serde_json::from_str` would. Positive numbers are u64 and negative numbers i64, and with `arbitrary_precision` they compare equal as strings.
- After the canonical check, Stellar accepts exactly the canonical, integer-only JSON objects that the standard path accepts, and each one means the same thing to `context_u64` and the rest of the evaluator.
- Contexts that aren't objects parse successfully but are then rejected by `as_object()`.
- Floats are still rejected on Stellar; that difference is documented.

## Call site and bindings (`common/src/lib.rs:403-420`)

- The generic `serde_json::from_str` is compiled out when `integer-json` is enabled.
- The 1024-byte check, the exact canonical re-encoding check, the "non-empty context requires evidence" rule, the evidence checks and `request_hash` over the exact bytes are all unchanged.
- Nothing about policy execution or evidence binding is weakened.
- The removed post-parse validator is fully covered by the parser, which enforces the same depth, entry and integer rules during parsing.
- The new attestation test fails the escaped-emoji, trailing `{}`, duplicate-key and changed-value variants, each with `InvalidEvidence`.

## Float removal

The only other deserializer in the Stellar path is `serde_json::from_slice` in the JSON artifact path (`common/src/lib.rs:305`). Stellar never calls it, so the release build's LTO should remove it. The SDK's evaluation and validation code deserializes nothing. Whether the final Wasm is actually free of float opcodes still has to be confirmed.

## Still pending

- The new Soroban CI is still running. It has to show that the optimized Wasm has no float opcodes and that upload, activation and execution fit the unrelaxed budgets.
- There is still no public-chain deployment or wallet acceptance.

## Compact SHA-256 and exact-artifact build selection

Candidate: `0cf443c21b259f4935962ad7a7e1a708727d2940`. Review response SHA-256: `e9237e3fc1b5d77ecb66d018bfbc28f90085c46efa3d0372ed8ad4a5f268d643`.

I found no material findings in `e7ccdc4..0cf443c`, so this delta closes clean. This was reviewed on Claude Opus 5.5 (`claude-opus-5-5`). HEAD is `0cf443c21b259f4935962ad7a7e1a708727d2940`. The only untracked file is `contracts/review/stellar-transport-opus-5-5.md`, the preserved record of my earlier reviews. Nothing was built or edited.

## Same hash algorithm
- I read the pinned `sha2-0.10.9/src/sha256.rs`. The `force-soft-compact` branch is checked first, so every target uses `soft_compact::compress`, including the native test hosts. The native Stellar tests therefore run the same SHA-256 code as the Wasm build.
- The Stellar lockfile has exactly one `sha2`, version 0.10.9, which is also the version the SDK depends on (`Cargo.toml:26`). The lockfile change only adds `sha2` to the Stellar package's dependency list.
- The three test vectors in `stellar/src/test.rs` are the standard SHA-256 test values: the empty string, `abc` (one block) and the 448-bit message (padded to two blocks). Without them, the fixture digests and the contract's digests could be wrong in the same way and still agree, because both use this implementation. The vectors break that circularity.
- On chain, only the SDK's `digest` uses this code: artifact and IR hashes, the request hash and semantic keys. The host `env.crypto().sha256` doesn't.

## Feature isolation
- The feature exists only in the Stellar workspace. `common`, `solana` and `sbf-tests` each have their own workspace and lockfile, and the SBF build uses the syscall.
- In native Stellar test builds, the host crates that use `sha2` also get the compact implementation. That only affects speed, not correctness or budget metering.

## Artifact validation and budgets (`contracts.yml`, `test.rs:429-451`)
- **Variant selection:** CI builds LLVM `z` and `s`, and runs each through Binaryen `-Oz`, `-O2 --shrink-level=2` and `-O4 --shrink-level=2`. It keeps the smallest by bytes, logs every size and the selection, then records the sha256.
- **Exact bytes tested:** that exact file is the one passed to the VM test through `ALLOWIT_STELLAR_WASM`.
- **Upload and deploy:** these run after `reset_default()` under the unchanged default budget and the SDK's default mainnet invocation limits. A failure is logged and re-raised, so the test fails. Nothing relaxes a limit or disables enforcement.
- **Activation and execution:** each gets its own reset under the same limits. That includes the maximum 8192-byte semantic policy and the user-input rollback case.

**Not a defect:** CI picks the variant by size, not by CPU. If the smallest build fails upload or execution, CI fails even if another variant would have passed. That's conservative, and it can't produce a false pass.

## Still pending
- The new Soroban target CI on this commit hasn't finished. It has to show that upload and deploy come in under 100M CPU and that activation and execution pass under mainnet limits.
- There is still no public-chain deployment or wallet acceptance.

## Stellar resource profile and separate upload metrics

Candidate: `20635a472d684fd75dfa253c64d65ee7cff027f5`. Review response SHA-256: `bf469423a86602de72a61efb7688d3747c43b84eb70ef582702a9e8bda52fb3e`.

I found no material findings in `0cf443c..20635a4`. This was reviewed on Claude Opus 5.5 (`claude-opus-5-5`). HEAD is `20635a472d684fd75dfa253c64d65ee7cff027f5`, the six-file delta you described. The only untracked file is the preserved review record, `contracts/review/stellar-transport-opus-5-5.md`. Nothing was built or edited.

## Profile enforcement
With the `stellar` feature, `MAX_CHAIN_ARTIFACT_BYTES` is 4096 (`common/src/lib.rs:18-21`). All of the size checks use that constant, so each is 4 KiB on Stellar:
- the raw-size check in `activate` (`stellar/src/lib.rs:177`)
- the decoder's check on the binary wire size
- the check that the rebuilt canonical JSON fits, in `validate_binary_artifact`
- `binary::encode`

Execution runs `validate_binary_artifact` again, so a larger artifact can't be admitted at activation or run at execution. The 256-node and depth-8 limits, the hashes and the evaluator are unchanged, and no signer, evidence or policy-effect check changed.

## Feature isolation
- Only `stellar/Cargo.toml` enables `stellar`.
- `solana`, `sbf-tests` and the default `common` build are separate workspaces or feature sets, so they keep 8192.
- CI now runs the common tests with default features (8192) and again with `--features stellar` (4096).
- If the CLI is run without `stellar`, it can emit a larger binary artifact. Stellar then rejects it at activation, which fails closed. The README now tells users to build with `compiler,stellar`.

## The boundary tests aren't bypassed
- **Maximum fixture:** `maximum_semantic_fixture` is built with the Stellar feature set, so it fills to exactly 4096 bytes of canonical JSON while keeping the depth-8 semantic structure. The new test asserts the 4096 length.
- **The new 4097-byte rejection test** (`test.rs:523-542`):
  - It adds one byte to the intent and the intent stays under 2048, so the JSON size check is what rejects it, not the intent limit.
  - Binary framing and the IR hash stay valid.
  - The binary is well under 4096, so the raw-size check at line 177 doesn't fire first.
  - The authenticated digest is updated, so the rejection can't come from a hash mismatch.
- **Upload, deploy, activate and execute** each start with `reset_default()`, and every one of those calls fails the test if it goes over:
  - `upload_contract_wasm` is now measured on its own, which removes the earlier registration metric that was hiding the upload cost.
  - `deploy_v2` is measured separately.
  - Activation and execution are held to the SDK's mainnet invocation limits, with no budget raised or disabled.

## README accuracy
The README (`contracts/README.md:65`, first sentence of the Stellar paragraph) says Stellar's 4 KiB profile "is based on measured execution cost under its 100M CPU limit". The measurement so far shows only that 8 KiB exceeded 100M. No run has yet shown that 4 KiB fits. This is a Low wording issue, not a source defect. A minimal fix would be: "Chosen after the 8 KiB maximum exceeded the 100M CPU limit; the 4 KiB maximum must be confirmed by the compiled-Wasm VM test." Every other size statement matches the code.

## Still pending
- The compiled-Wasm VM run on this commit hasn't happened. It has to show upload, deploy, activation and execution of the maximum 4 KiB, depth-8 semantic policy under 100M CPU and mainnet limits, plus the user-input rollback.
- There is still no public-chain deployment or wallet acceptance.

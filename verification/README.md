# Allowit verification

This folder holds verification for `core/`. [`lean/AllowIt.lean`](lean/README.md) is an executable, machine-checked Lean model of feature selection and a small policy decision language. It uses the five core feature identifiers. Its 13 theorems establish only their bounded model: no proof or differential test connects the model to the Rust implementation.

The Solana verification set is in [allowit-contracts-solana `verification/`](https://github.com/allowit-hq/allowit-contracts-solana/tree/main/verification): the obligation ledger, the native daily-rule model and its source-bound check, the published-source check, and the refinement, adapter, trace, refusal, isolation and runtime evidence. Those files bind each other by hash, so they moved as one unit.

## Evidence rules

- Specify the intended claim before reviewing its proof. Include permitted cases and a successful witness as well as forbidden cases.
- Bind every result to its domain, configuration, code, specification and toolchain. Recheck affected evidence after changes.
- A valid Lean proof establishes its formal statement. A production claim additionally needs an implementation connection and explicit adapter/compilation assumptions.
- Unknown, unsupported, missing or stale evidence remains visible. A count of passing tests or theorems is not a completeness score.

## Run

The unit tests need only Python 3. They test the checker's audit logic with a stand-in Lean executable:

```sh
python3 -m unittest discover -s verification/tests
```

The proof check needs Lean 4.11.0 (`lean/lean-toolchain`):

```sh
LEAN_BIN=/absolute/path/to/lean python3 verification/lean/check.py
```

# Oracle workflow execution trace review

Reviewed against SDK baseline `19152cb`. The opt-in oracle trace records actual top-level execution and hoisted configuration checks, including early returns, semantic evidence requests and owner input. Normal decisions and contract feature profiles retain their existing behavior.

Independent Claude Code review used the explicitly selected and verified model `claude-opus-5-5`, session `403c8daa-6d58-4f79-8e84-97f72cdbd325`. The reviewer received only an SDK snapshot, found no material correctness or security issues, and reported no merge blockers. Its local receipt is `/private/tmp/allowit-sdk-workflow-review/result.json`; that temporary path is not required to build or use the SDK.

The reviewer ran 57 native tests, the no-default-features contract profile, a std/oracle-ledger profile without compiler support, and Clippy. Coordinator validation also passed the native suite, all supported feature profiles, warning-free Clippy, the actual WebAssembly ABI/trace checks, common contract conformance and Stellar codec tests, native Solana transfers, and native Soroban tests. Compiled SBF and Soroban VM/resource verification remains the responsibility of the source commit's Contract runtimes CI; native tests alone do not establish those results or public-chain deployment.

Nonblocking limitations: custom blocks describe only the path taken, not all nested branches; context rejection can produce a terminal trace with no visited or failed policy node; missing generic confidence evidence is a terminal failed check, while only missing semantic evidence is a resumable `verifying` check. The recorder's bounded linear node lookup relies on the freshly compiled workflow projection.

Reviewed file SHA-256 values:

```text
9d3a0146fb0ca1f99b456741a99ae792276080683fa9dbff6020783c578355eb  README.md
2424f3d7892f832627b9a4585a808eda1ef63530594704bfe3106a15750efc19  scripts/test-wasm.mjs
dbf3250506a9431136031caf6181d3fb13bc46afbf3f025867f2c6b9f5334a23  src/lib.rs
f14d7420f35a0395e2ee1403da1a17ac35a600cb91ca273973d3c9ccc4915090  src/protocol.rs
a239884e98e177df58ea76487783acf36a87095e62de985cbf7448ed6640f658  src/runtime.rs
15ebde9ba6400a1fcc21b234a98c1fdb53b68a164f88b41efb1f2ebaea5e859a  src/types.rs
b8d4a89465dfc825f5a1820ddf2203bf7208c6b144e64c708a559af72331a626  src/trace.rs
d3710e64e9be398b0ef15e619a6c8b01cb57456f0cb6dfd5673d4377e7ed4d89  tests/workflow_trace.rs
```

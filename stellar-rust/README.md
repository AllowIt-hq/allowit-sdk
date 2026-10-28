# Stellar wallet SDK

This crate maps the canonical `ExecutionRequest` to the admitted `stellar::sep41_transfer` profile. Native classic custody checks the current account weights and applicable transaction and payment thresholds. The 3/1/1 weights and 2/2/3 thresholds are deployment templates, not admission requirements. ODC custody calls its generated public `execute` wrapper with the private `_execute` entrypoint, exact native arguments, installed executable, schema and source identity.

The same authored policy source passes `compile_stellar_policy` before either route signs. `stellar-native POLICY.rs` emits static Rust checks for the ODC artifact. The source profile admits local asset, recipient and monetary guards, boolean control flow and explicit rejection. Unsupported or external-evidence expressions fail admission. USDC helper monetary constants retain their declared six-decimal meaning and lower to seven-decimal SAC comparisons. Native request amounts and counters remain exact `i128` base units. Raw source integer amount fields are rejected because their unit projection is ambiguous. XLM supports admitted asset and recipient guards; the USDC financial helpers cannot be used for XLM.

Use `rpc::trusted_state_for_discovery` for public network, account and ODC binding discovery without requiring a formerly configured customer signer. Its `DiscoveredState` reports both live authority and whether an ODC is paused. Discovery does not authorize an operation. `rpc::trusted_state` checks the presented customer key against current authority. Native accounting must come from the durable policy coordinator.

The current ODC agent account can have multiple signers. The customer signing key is `config.agent`; the ODC transaction source and fee payer are the current agent account in `state.source.address`. Additional Ed25519, hash, preauthorized and signed-payload keys do not lock out a sufficient current customer quorum.

Native authorization verifies both customer and engine role proofs. Exact quorum checks include matching transaction preauthorizations; ODC checks use the final hash after resource simulation. The transaction envelope contains only the signatures needed by current chain thresholds, including any matching preauthorization, so unused signatures cannot invalidate it. High-threshold administration and owner weights are reported from live state rather than imposed by this transfer profile.

`prepare`, `sign` and `authorize` bind the exact request and signatures. Native transactions bind their operation ID in the hash memo. Soroban transactions use no memo and bind the same identity inside the signed `execute` arguments.

New native bounds start at zero and end at the exact approved expiry. Retained native bounds may start at their original creation time. Validation preserves those signed bytes for status and refuses new authorization.

ODC preparation needs resource simulation before signing. Native configuration permits at most 1,000,000 stroops (0.1 XLM) in fees. The ODC ceiling is 2,000,000 stroops (0.2 XLM). The exact request must cover the installed fee bound. Simulated resource fees plus inclusion fee must fit that bound. Preparation never increases it.

`validate_retained_prepared` and `validate_retained_signed` verify the original transport, effects, hashes and signatures without compiling policy source. Use them only for read-only history and original-hash recovery. Native transaction signatures do not prove the accompanying policy source metadata. Before calling these helpers, compare that metadata with the original owner-approved binding in the durable record or client token. New preparation, signing and first broadcast still require current policy admission and fresh transaction bounds.

`rpc::submit` retains signed bytes before broadcast. `recover` queries the original transaction hash. An uncertain broadcast never authorizes a replacement payment or releases its reservation. Terminal settlement remains distinct from external delivery.

The signing seed stays with its actor. Wallet configuration contains public installation data. Client-held signing material must remain private and is not part of public wallet configuration. Production promotion is a separate operation.

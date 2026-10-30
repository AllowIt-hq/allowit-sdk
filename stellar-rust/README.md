# Stellar wallet SDK

This crate maps typed execution requests to classic payments and policy-wallet transfers or SEP-56 deposits. The public wallet `execute` wrapper keeps custody checks separate from the complete policy.

`vault_deposit_request` binds the underlying debit and minimum share credit. The policy wallet is the source, operator and share receiver. `recipients` constrains the destination vault. Shares retain the vault's decimal scale. The installed underlying SAC uses seven decimals.

Deposit principal remains temporary loss until verified redemption. Fees and conversion loss count as expenses. Receipt verification binds the original transaction, committed events, underlying transfer and share mint. A balance or valuation alone cannot prove settlement. `vault_position_observation` reads the selected vault's `preview_redeem` value. That report does not prove future redemption liquidity.

`vault_redemption_receipt` verifies an owner-authorized redemption, share burn and net underlying credit. The server can reconcile that receipt against the retained position. Delegated redemption and direct classic-account vault calls reject because they lack an admitted atomic minimum-output wrapper. The SDK does not bypass agent authorization.

## Source and configuration

One `_execute(ctx: &Context) -> PolicyResult` contains the full policy. Standard wallets enforce only its initial consecutive same-name Grant-rule calls. The server evaluates every reached statement. See [the complete vault source](../examples/stellar-vault-policy.rs).

`WalletConfig.source_hash` binds the complete source. `prefix_digest` binds the standard module, asset, schemas and ordered rule prefix. Standard policies require an explicit `source_instance` for offline source approvals. The native `policy_instance` remains the real deployed instance. Accounting stays under the physical wallet and asset.

`source_updates` verifies the current owner's offline signature quorum and exact next source revision. A standard prefix change requires native reconfiguration. A suffix change with the same prefix needs no chain transaction. The client keeps its trusted bundle hash until an owner-approved public bundle refresh. Contract-account owners need a separate offline signature adapter.

`admin::PolicyConfig::for_policy` builds fresh constructor configuration. `OwnerAdmin` reads the actual owner and binding before it prepares owner controls. Pausing and recovery do not depend on recompiling retired offchain source. Owner Grant updates preserve nonce and protected history.

`TrustedState.history` exposes native conservative expense history, including operations whose selected rule inputs were Null. The server separately retains actual settled expenses, temporary loss and gains. Neither Grant changes nor source revisions reset financial history.

## Preparation and signatures

The service supplies protected `Context.rule_facts` and actual accounting to `rpc::prepare_with_policy_context`. Use `authorize_server_with_policy_context` for a policy wallet and `sign_with_policy_context` for native service signing. Every context must match the exact request, source, asset, destination and preparation time. A standard Grant comes from fresh native storage. Classic accounts have no Grant map. A reached Grant reference on that route refuses execution.

`rpc::estimate_operation_fee` returns only an unsigned fee estimate. Its private zero-gas probe cannot produce an approved or signable plan. The service rebuilds the expense bounds, repeats the full policy and simulates the exact request. Server authorization can increase resource costs. The service retains and reserves every real authorization before enforce-mode simulation. It repeats this process within a bounded loop and exposes agent bytes only after the fee stabilizes.

New signing requires the final outer fee to equal the signed attributable gas bound and remain within configured and typed ceilings. These checks depend on the server and client expense attestation. The contract cannot inspect arbitrary outer transaction fees. Underlying custody, output bounds and replay checks remain native.

The agent then revalidates the exact packet through its trusted RPC and signs the final transaction. Current native account signers and thresholds determine the required quorum. Owner controls retain their separate authority. No helper submits during preparation.

Canonical `expiry` records the minimum deadline actually reached in a successful evaluation. Null and unreached rules add no deadline. Approval metadata clips transaction time bounds. Other custom time conditions apply when the policy evaluates.

Native transactions bind the full source hash and exact typed request in their hash memo. Soroban requests bind full source and expense fields inside native authorization. Expense maps use canonical XDR bytes. Sponsored x402 fee attribution remains explicit and separate from direct agent-paid gas.

## Receipts and recovery

`rpc::transaction_accounting_receipt` verifies the original envelope, result, ledger close time, charged fee and payer. Fee-bump and multiplexed fee attribution require separate adapters. Cross-asset gas accounting requires an authenticated conversion source. Unknown costs remain unpriced rather than becoming zero.

`validate_retained_prepared` and `validate_retained_signed` preserve original bytes without compiling source. Compare them with the original durable owner-approved record. They authorize status and original-hash recovery only. New signing rejects historical request formats.

`rpc::submit` retains signed bytes before broadcast. An uncertain response preserves the original transaction hash and reservation. Terminal settlement remains separate from delivery.

Signing seeds remain with each actor. Public bundles contain configuration and identity. Direct execution needs both current agent and service authority. x402 requires owner enablement and both exact raw-key approvals. Its encoding helpers do not establish policy approval.

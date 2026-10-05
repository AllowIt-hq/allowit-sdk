# Devnet paid-API vault v1

This is a separate fixed-policy wallet alongside the existing IR allowance adapter. It enforces owner-reviewed total allocation, per-payment cap, expiry, executor and recipient; it does not execute arbitrary source or IR. Source and IR digests are provenance bound by the owner and compiler, not a claim of on-chain source compilation. HTTP origin, resource and challenge authenticity remain the executor and gateway responsibility.

Only explicit Devnet builds accept this instruction namespace. The host must verify the cluster genesis and frozen deployment code hash. Canonical Circle Devnet USDC and classic SPL Token are the only asset/rail. No arbitrary CPI, owner-input bypass, semantic policies or owner-wallet allowances are exposed.

## Wire and accounts

Solana program prefix 0xA1 routes separate fixed-policy vault module, legacy variants unchanged. PDA state seeds `[b"allowit-vault",owner[32],policy_id[32]]`; state is vault token authority. Classic token ATAs derived with standard Associated Token program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL. Mint fixed Circle Devnet USDC. Reject this wire on non-devnet program builds.
All integers LE u64; keys/hashes raw 32 bytes; no Borsh string lengths. Initialize `[A1,00] + policy_id + executor + compiler + recipient_wallet + service_hash + source_hash + ir_hash + allocation + per_call + expires_unix`. Accounts: state(w),owner(s,w),compiler(s),ownerATA(w),vaultATA(w),recipientATA(ro),mint(ro),tokenProgram(ro),Clock(ro),System(ro),Rent(ro). Create vault ATA idempotently in preceding standard instruction. Program creates state PDA using owner rent, validates accounts, transfers exactly allocation ownerATA->vaultATA atomically. Owner and compiler sign same message. Existing state cannot reinitialize.
Execute `[A1,01] + amount + next_nonce + challenge_hash + request_hash + challenge_expires_unix`. Accounts: state(w),executor(s,w),vaultATA(w),recipientATA(w),mint(ro),tokenProgram(ro),Clock(ro),chargePDA(w),System(ro),Rent(ro). chargePDA seeds `[b"allowit-charge",state[32],challenge_hash[32]]`. Markers created atomically, executor funds rent. Only one transfer_checked CPI. Monotonic nonce exactly +1; zero hashes/amount rejected; require now< both expiries. Gateway verifies challenge_hash = SHA256(challenge.id UTF8), request_hash canonical binding (defined by app), expiry and marker/account keys plus inner transfer. Program does NOT assert web request authenticity: executor authenticates HTTP bindings, chain enforces funds constraints.
Revoke `[A1,02]`: state(w),owner(s). Idempotently sets revoked. Withdraw `[A1,03]`: state(w),owner(s),vaultATA(w),ownerATA(w),mint(ro),tokenProgram(ro). Revoke+transfer all remaining atomically. Keep state, token account and markers (no rent reclamation in v1).
State length 305: magic `ALVLT001` (8), policy_id,owner,executor,compiler,recipient_wallet,service_hash,source_hash,ir_hash (8*32),allocation,per_call,expires_unix,spent,nonce (5*8),revoked (1 byte,0/1).
Marker length 120: magic `ALCHG001` (8),state(32),challenge_hash(32),request_hash(32),amount(8),nonce(8). No marker close or reuse.
Helpers must check actual serialized full tx<=1232. Activation can have 2 or 3 signers; executor is fee payer for backend executes. Chain genesis and deployed program freeze are host verification.

## Verification

Run `cargo test --locked --manifest-path contracts/solana/Cargo.toml`. Build the actual program with the pinned toolchain in `contracts.yml`, then run `SBF_OUT_DIR=... cargo test --locked --manifest-path contracts/sbf-tests/Cargo.toml --test vault`. Those tests execute real System and SPL Token CPIs and verify failed-instruction rollback, funding, exact budget limits, duplicate challenges, signer/account restrictions, revocation and withdrawal.

Native tests are not public-chain evidence. Deployment and consumer-wallet acceptance must record program ID, artifact hash, network and finalized transaction signatures separately. Retained state/markers and token-account rent are not reclaimed by v1 withdrawal.

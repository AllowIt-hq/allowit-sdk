# Allowit Tempo import

*A stable import path for the Tempo vendor crate.*

`allowit-tempo` re-exports the vendor crate of the same name from an exact revision of [allowit-contracts-tempo](https://github.com/allowit-hq/allowit-contracts-tempo/tree/fac59366570b86e4ea5cedd98216a713ce9851c8/rust). The vendor owns plan verification, native transactions and executor recovery; this package only pins the revision so existing verifier consumers keep working without changing their imports.

New SDK clients should use the [client facade](../client/) instead. The [Tempo vault lifecycle](../docs/tempo-vault.md) note explains where the pieces sit.

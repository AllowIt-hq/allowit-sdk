# Tempo compatibility import

This package reexports the `allowit-tempo` vendor crate from an exact revision
of [the Tempo contracts repository](https://github.com/AllowIt-hq/allowit-contracts-tempo/tree/fac59366570b86e4ea5cedd98216a713ce9851c8/rust).
The vendor owns plan verification, native transactions and executor recovery.
SDK clients use the client facade. Existing verifier consumers can retain this
import path.

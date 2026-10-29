# Tempo compatibility import

This package reexports the `allowit-tempo` vendor crate from an exact revision
of [the Tempo contracts repository](https://github.com/AllowIt-hq/allowit-contracts-tempo/tree/8e44469a4431fbe94afd8dd2049f2f062663d39c/rust).
The vendor owns plan verification, native transactions and executor recovery.
SDK clients use the client facade. Existing verifier consumers can retain this
import path.

# Tempo compatibility import

This package reexports the `allowit-tempo` vendor crate from an exact revision
of [the Tempo contracts repository](https://github.com/AllowIt-hq/allowit-contracts-tempo/tree/41df6374f016421af68a6a64d6cca251a469edb8/rust).
The vendor owns plan verification, native transactions and executor recovery.
SDK clients use the client facade. Existing verifier consumers can retain this
import path.

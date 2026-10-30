//! PaySH connection: the pinned Solana vendor crate owns the protocol.
//! Backs `allowit paysh`; `TOKEN_ENV` names `ALLOWIT_PAYSH_TOKEN`, which output redaction removes.
pub(crate) use allowit_solana_vendor::paysh::{TOKEN_ENV, run};

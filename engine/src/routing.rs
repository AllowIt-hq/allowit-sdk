//! Explicit network selection never changes process environment or inferred authority.
use crate::error::{Error, Result};
use std::cell::RefCell;
thread_local! { static SELECTED: RefCell<Option<String>> = const { RefCell::new(None) }; }
pub fn normalize(network: &str) -> Result<&'static str> {
    match network {
        "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1" | "solana:devnet" => Ok("solana:devnet"),
        "solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z" | "solana:testnet" => Ok("solana:testnet"),
        "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp" | "solana:mainnet" => Ok("solana:mainnet"),
        "eip155:42431" | "tempo:testnet" => Ok("tempo:testnet"),
        "eip155:4217" | "tempo:mainnet" => Ok("tempo:mainnet"),
        "stellar:pubnet" | "stellar:mainnet" => Ok("stellar:mainnet"),
        "stellar:testnet" => Ok("stellar:testnet"),
        _ => Err(Error::usage(
            "Unsupported network identifier; use an admitted x402 CAIP-2 network",
        )),
    }
}
pub fn caip2(network: &str) -> Result<&'static str> {
    match normalize(network)? {
        "solana:devnet" => Ok("solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1"),
        "solana:testnet" => Ok("solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z"),
        "solana:mainnet" => Ok("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"),
        "tempo:testnet" => Ok("eip155:42431"),
        "tempo:mainnet" => Ok("eip155:4217"),
        "stellar:mainnet" => Ok("stellar:pubnet"),
        _ => Ok("stellar:testnet"),
    }
}
pub fn selected() -> Option<String> {
    SELECTED.with(|n| n.borrow().clone())
}
pub fn with_network<T>(network: Option<String>, f: impl FnOnce() -> Result<T>) -> Result<T> {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SELECTED.with(|n| *n.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(SELECTED.with(|n| n.replace(network)));
    f()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_is_scoped() {
        with_network(Some("tempo:testnet".into()), || {
            assert_eq!(selected().as_deref(), Some("tempo:testnet"));
            Ok(())
        })
        .unwrap();
        assert!(selected().is_none());
    }
}

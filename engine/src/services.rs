//! Typed service facade. Concrete protocols live in their owning rail packages.
pub use allowit_sdk::service::*;
pub use allowit_sdk::transport::http;
pub use allowit_solana_delegated_x402 as x402;
pub use allowit_solana_delegated_x402 as delegated_x402;
pub use allowit_solana_paysh_cooperating as cooperating;
pub use allowit_solana_service_upstreams::catalog;
pub use allowit_solana_service_upstreams::{upstream_from_env, validate_service_input};
pub use allowit_tempo_services::transaction as tempo_transaction;
pub use allowit_vendor_paysh_stock as stock;
pub mod rpc {
    pub use allowit_sdk::service::rpc::Operation;
    pub use allowit_sdk::transport::rpc::*;
    pub use allowit_solana_service_rpc as solana;
    pub use allowit_tempo_services::rpc as tempo;
}
